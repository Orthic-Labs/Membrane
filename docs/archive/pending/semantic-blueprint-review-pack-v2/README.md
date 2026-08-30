# Membrane Semantic + Blueprint Review Pack v2

> **Status:** Fable review draft — not canonical.
> **Date:** 2026-08-29
> Existing canonical subsystem documents win on conflict. Re-derive implementation state before execution.

**Imported source:** `membrane-semantic-blueprint-review-pack-v2.zip`
**Source SHA-256:** `D18F33CB3CE375FE1D118DF262288A8A9EDEA321CD73B1A9BDF3EF2C19C7F634`
All eight source documents were placed under pending. This README and the Adapt amendment contain
Membrane-side reconciliation additions for Fable; no proposal was promoted to canon.

## Purpose

This pack contains two related but independent proposals:

1. **Source-bound semantic compilation:** let Cortex retain useful semantic knowledge derived from Ledger-indexed documents while preserving exact provenance back to Ledger.
2. **Blueprint architecture-view hardening:** absorb the useful engineering ideas found in Archify without adopting Archify or any third-party runtime.

Nothing here adds a seventh Membrane subsystem.

## Review order

Read in this order:

1. `00-OVERVIEW-SOURCE-BOUND-SEMANTIC-COMPILATION.md`
2. `01-LEDGER-SEMANTIC-PRODUCER-AMENDMENT.md`
3. `02-CORTEX-DOCUMENT-SEMANTIC-KNOWLEDGE-AMENDMENT.md`
4. `03-PULL-SEMANTIC-AND-SOURCE-FUSION-AMENDMENT.md`
5. `04-ADAPT-DOCUMENT-COMPILATION-BOUNDARY-AMENDMENT.md`
6. `06-BLUEPRINT-ARCHIFY-ABSORPTION-AMENDMENT.md`
7. `05-EXPERIMENT-LOCAL-SEMANTIC-WORKER.md`

The model experiment is last intentionally. Model choice must not drive subsystem ownership.

## Fit against current canon

| Proposal | Fit | Review state |
|---|---|---|
| Ledger structural nodes & exact source resolution | Matches Ledger's source-positioned AST, stable node identity, bounded structural expansion & rebuildable-projection ownership | New delta/evidence wire shapes remain pending |
| Cortex document-derived knowledge | Matches Cortex admission/lifecycle ownership & existing allowance for durable knowledge to cite Ledger nodes | Candidate classes, automatic-admission policy & revalidation contract remain pending |
| Pull semantic/source fusion | Matches Pull's final provider admission, sufficiency & budget ownership | Concurrent vs lazy/risk-class verification requires qualification |
| Adapt boundary | Matches canonical behavioral-learning boundary: generic document extraction is not Adapt | Updated here with pipeline observations, detector coverage & hard negatives |
| Local semantic worker | Fits only as an optional bounded producer experiment | No resident model, model family, scheduler or deployment is approved |
| Blueprint architecture hardening | Fits Blueprint evidence, stable identity, derived-view, atomic-publication & impact boundaries | `ArchitectureFlowViewV1` schema exists; production reach was not established here. Structure/delta/digest/diagnostic additions remain pending |

## Reconciliation constraints

1. `Cortex admission producer` does not by itself assign tray/daemon scheduling or Ledger-delta
   orchestration to Cortex. Fable must close runtime ownership separately.
2. `LedgerEvidenceRefV1`, `SemanticSourceDeltaV1`, `DocumentKnowledgeCandidateV1`,
   `ArchitectureStructureViewV1`, `ArchitectureDeltaViewV1`, digest splits & diagnostics are proposed
   contracts, not landed surfaces.
3. Existing Blueprint atomic generation/last-known-good doctrine is reused. New architecture-artifact
   publication work starts with a producer audit to avoid duplicating landed behavior.
4. Adapt evaluates pipeline behavior from typed observations. It neither compiles documents nor owns
   generic traces, compiler scheduling, admission, retrieval, or policy activation.

## Proposed architecture in one page

```text
documents
   |
   v
 Ledger
 source identity + structural nodes + exact resolution
   |
   +-------------------> Pull ----------------------+
   |                    exact source candidates     |
   |                                                v
   +--> semantic compiler --> Cortex admission --> Pull
        derived proposals      durable knowledge    |
                                                   v
                                                 context
```

Key invariant:

> **Cortex may remember what a document says; Ledger proves where the current document says it.**

Separately:

```text
repository state
      |
      v
  Blueprint truth
      |
      +--> ArchitectureFlowViewV1       (existing)
      +--> ArchitectureStructureViewV1  (proposed)
                 |
                 +--> semantic/evidence/projection digests
                 +--> ArchitectureDeltaViewV1
                 +--> disposable generated views
```

## Decisions Fable should attack

These are the real architectural decisions, not implementation details:

1. **Compiler ownership:** Is a Cortex-owned admission producer the cleanest home for document semantic compilation, or should another existing Membrane boundary own orchestration?
2. **Automatic admission:** What semantic classes, if any, may cross Cortex admission without review? Precision should dominate recall.
3. **Revalidation:** Is `LedgerEvidenceRefV1` sufficient to invalidate/revalidate derived knowledge when source structure moves or changes?
4. **Pull policy:** Should source verification be concurrent, lazy/on-demand, or risk-class dependent?
5. **Structural granularity:** Is “smallest structurally coherent sufficient region” precise enough for Ledger/Pull qualification?
6. **Resident model economics:** Does a local sub-1B worker earn its RAM/compute cost versus lazy batch inference or no model?
7. **Blueprint digests:** Are separate semantic/evidence/projection identities the right split, and can they remain stable through synthesis algorithm upgrades?
8. **Blueprint delta semantics:** Are `semantic_changed`, `evidence_changed`, and `projection_changed` sufficient, or are more authority/freshness states needed?
9. **Execution observability:** Are the Adapt amendment's compilation observations, budgets,
   terminal-state accounting & hard-negative detectors sufficient to expose worker waste without
   treating abstention, cache reuse, or negative corpora as failure?

## Hard non-goals

- No OpenKB-style generated wiki as authority.
- No LLM-generated truth inside Ledger.
- No direct unchecked Ledger -> Cortex writes.
- No generic document extraction routed through Adapt.
- No arbitrary fixed-size Markdown chunking.
- No Archify dependency, renderer, viewer, runtime, or schema.
- No agent-authored architecture topology treated as Blueprint truth.
- No conflation of Blueprint `route`, `reach`, and `impact`.

## Expected review output

Fable should ideally return:

- `accept`;
- `accept with amendment`;
- `reject`;
- unresolved architecture decision requiring evidence;

for each numbered decision above, plus any ownership conflicts with current canon.

## Fable review — 2026-08-29

Overall verdict: **accept with amendments**. No canon-invariant conflicts found. This pack does not
supersede `../MEMBRANE-PENDING-IMPLEMENTATION.md`; that document remains the sole schedulable
ledger, and accepted decisions below become schedulable only as entries there.

Per-decision verdicts:

1. **Compiler ownership — accept with amendment.** Cortex owns candidates and admission, as
   proposed. Runtime orchestration is already specified and must not be re-invented: scheduling
   belongs to the tray-owned daemon's `BackgroundReviewContract` (pending doc §4 — gates,
   single-flight, budgets, fail-closed config, cancellation) with semantic compilation as a new job
   kind, executed through the §13.2 authenticated loopback provider seam ("one provider seam …
   no second model stack"). No new coordinator, no Ledger- or Cortex-owned scheduler.
2. **Automatic admission — accept with amendment.** Default every class to proposal/quarantine.
   Promotion reuses the §16.1 `pending → approved → admit_approved_proposal` consumer; do not build
   a second promotion path. Auto-admission of any class requires precision-first frozen
   qualification (cross-subsystem canon §13), with precision and unsupported-claim rate dominating
   recall.
3. **Revalidation — accept.** `LedgerEvidenceRefV1` is sufficient given the §18 structural
   span-hash identity unification (span-hash is identity; slug/ordinal anchors are aliases). The
   typed resolve outcomes (`current | relocated | stale | missing | denied`) stand.
4. **Pull policy — unresolved, requires evidence.** Risk-class-dependent verification is the
   leading candidate; decide by qualification, not doctrine. Wire the choice through the
   §13.1 `SufficiencyContractV1` path rather than a new mechanism.
5. **Structural granularity — accept.** "Smallest structurally coherent sufficient region" is
   qualifiable via the proposed receipts (`expansion_hops`, `context_savings_ratio`).
6. **Resident model economics — unresolved, requires evidence.** The experiment doc's own instinct
   is endorsed: revision-triggered work makes permanent residency economically suspect. Default is
   no local model until arm D beats arm B on the frozen corpus.
7. **Blueprint digests — accept.** Three-digest separation is correct; stability through synthesis
   upgrades is carried by `projectionDigest` capturing algorithm version, as specified.
8. **Blueprint delta semantics — accept with amendment.** `ambiguous_identity` stands; align
   freshness/dirty-overlay states with Blueprint's existing typed staleness rather than adding a
   parallel vocabulary.
9. **Execution observability — accept.** Detector families plus hard negatives are well-formed;
   coverage receipts (`InsightDetectorCoverageV1`) are required beside every finding. The
   2026-08-29 incident analysis in `../ADAPT-HARNESS-EFFICIENCY-INSIGHTS.md` §3 is confirmed
   accurate by the integration owner of that dispatch: duplicate assignment execution and
   orchestrator role leakage did occur exactly as diagnosed.

Cross-cutting amendments:

- `DocumentKnowledgeCandidateV1.valid_from/valid_until` adopts the pending doc §16.2
  `TemporalValidityV1` vocabulary; no parallel temporal contract.
- Admission outcomes (`no_op | proposal | quarantine | reject`) reconcile with the §16.3
  write-time `duplicate`/`conflict` dispositions — one terminal-state vocabulary. Document-derived
  candidates flow through the same §16.3 near-duplicate pre-filter as every durable write.
- Qualification baseline for the codex "beat Ledger-only retrieval or delete it" gate is the frozen
  `docs/evidence/qualification/ledger-metrics.json` arms; the typed abstention shape (§17.1) must
  be honored by every arm, including semantic recall.
