# Blueprint v2 — Document Set

**Status:** Proposed governing set  
**Date:** 2026-09-04 (revised 2026-09-05)  
**Scope:** `Orthic-Labs/Membrane/blueprint`

This document set reconciles three inputs:

1. the current Blueprint implementation and its existing BPT canon;
2. the continuous-intelligence/runtime implementation audit;
3. the 251-capability donor synthesis across Infigraph, Potpie, GitNexus, CodeGraph, Sense, codebase-graph, Graphify, Glean, SCIP, Serena, Octocode, Kythe, stack-graphs, Joern, and Semantica;
4. the subsequent **Loom best-of synthesis**, used only to identify additional correctness, publication-safety and evaluation hardening—not to broaden Blueprint into memory/editing/deep-analysis ownership;
5. the **343-atom code-only capability matrix and Blueprint code-only gap audit**, used as implementation evidence only. It identified concrete current-code defects/refinements (relationship-vocabulary parity, duplicate SCIP semantics, extractor-cache fingerprinting, richer member/type resolution, JS/TS package semantics and domain identity) without changing Blueprint's ownership boundary;
6. the final **Membrane subsystem deep-dive (`membrane-blueprint.md`)**, used as the doctrine reconciliation pass. It confirmed several current canon exclusions and added four high-value refinements: descriptive convention mining, declarative/versioned resolution rules, emit-side semantic interchange, and generation-history temporal reads.

The set deliberately separates **what Blueprint is**, **what Blueprint owns**, and **what should be implemented first**.

## Governing order

When the documents appear to conflict, use this precedence:

1. **`01_BLUEPRINT_ARCHITECTURE_CANON_V2.md`** — architectural truth: ownership boundary, invariants, canonical data model, provenance and semantic authority.
2. **`03_BLUEPRINT_ATOM_AND_DECISION_REGISTER_V2.md`** — capability ownership and explicit decisions, including committed vs exploratory atoms.
3. **`02_BLUEPRINT_IMPLEMENTATION_PLAN_V2.md`** — execution order, file-level work, acceptance criteria, tests and SLOs.

`04_BLUEPRINT_DONOR_REFERENCE_V2.md` is **non-normative**. It maps donors to atoms/tasks, records licensing/cautions and tells implementation agents which repository to study first. If it conflicts with the three governing documents above, the governing documents win.

The earlier `BLUEPRINT-CONTINUOUS-INTELLIGENCE-IMPLEMENTATION.md` is **superseded as the master specification**, but most of its P0/P1 runtime work is retained in the new implementation plan.

## The one-sentence architecture

> Blueprint is a local-first, continuously fresh, evidence-backed repository intelligence system whose durable substrate is a canonical typed fact ledger; structural graphs, architecture/process views, search indexes and other analyses are rebuildable projections over that ledger.

## The one-sentence execution strategy

> Repair the existing canon and close the freshness/integration loop first; add semantic precision second; add richer derived intelligence third; add expensive retrieval sophistication only when benchmarks justify it.

## What changed from the previous implementation document

The previous implementation document correctly focused on the immediate product failure mode: a useful graph can still fail as a product if it is not automatically watched, discovered, queried and kept current. It therefore concentrated on `blueprint init`, watcher liveness, branch handling, bounded query-time repair, MCP resources, host setup and real resource SLOs.

The donor/canon reconciliation exposed a higher architectural layer that the previous document did not formalize:

- SCIP should be an actively orchestrated semantic producer tier, not just an import format.
- Semantic resolution needs an explicit, freshness-aware authority lattice.
- SQLite should be understood as the **canonical fact ledger**, not merely “the graph database.”
- Symbol identity and source occurrences must remain separate concepts.
- Process, architecture, contract, search and vector structures should be disposable projections.
- Tests, entry points, contracts, DI, ORM, configuration, RPC/tool handlers and UI navigation deserve typed provider contracts rather than being folded into one generic framework bucket.
- Multi-repo intelligence should cross explicit contract/bridge boundaries rather than merge repository node spaces.
- Retrieval sophistication must not weaken Blueprint Recall's evidence/admissibility discipline.

Those changes are now incorporated here.

The later Loom synthesis added three further hardening decisions without changing the architecture or stage order:

- **mechanized correctness**: schema/indexer behavior must be executable through source-anchored golden assertions and conformance verification;
- **publication safety**: an incomplete/partial extraction may not replace a known-complete generation, and incremental repair may not silently delete unrelated facts;
- **evaluation discipline**: correctness, retrieval and agent-use benchmarks use pinned fixtures/methodology, with regressions and negative results retained rather than hidden.

Its broader proposals—durable memory, semantic editing, mandatory vectors, always-on CPG/taint, confidence labels on authoritative facts, and a strict hook that blocks raw source reads—remain outside or contrary to Blueprint canon.

## Immediate implementation priority

The first tranche is intentionally narrower than the destination canon:

1. close existing partial/missing BPT atoms;
2. formalize the fact/projection/provenance contracts;
3. make setup and watcher readiness provable;
4. add Git-transition batching;
5. add bounded query-time dirty-file repair;
6. replace placeholder MCP resources with live repository-scoped resources;
7. install consistent host routing/hooks;
8. enforce actual resident watcher SLOs;
9. add cold-start repository orientation;
10. only then expand semantic producers/resolution and higher-order intelligence.

## Files

- `01_BLUEPRINT_ARCHITECTURE_CANON_V2.md` — normative architecture.
- `02_BLUEPRINT_IMPLEMENTATION_PLAN_V2.md` — execution-ready implementation plan.
- `03_BLUEPRINT_ATOM_AND_DECISION_REGISTER_V2.md` — atom status, sequencing and reconciled decisions.
- `04_BLUEPRINT_DONOR_REFERENCE_V2.md` — non-normative donor taxonomy, donor→BPT lookup, license/absorption guidance and repository-name disambiguation.

## Source/donor safety

Architectural ideas may be studied across all reviewed projects, but implementation absorption must remain license-safe. In particular, GitNexus is PolyForm Noncommercial and must be treated as design prior art unless a separate compatible license is obtained. stack-graphs is permissively licensed but archived/unmaintained, so critical use should be via an owned implementation/fork with Blueprint tests rather than an unexamined dependency.

The supplied comparative HTML labeled Graphify MIT, but the current `Graphify-Labs/graphify` repository `LICENSE` was directly verified on 2026-09-04 as **Apache-2.0**. The v2 donor record uses Apache-2.0; comparison artifacts do not override current repository license files.


## Code-only audit reconciliation

The code-only audit materially sharpens implementation detail but does not add destination atoms. Its strongest findings are treated as correctness/precision work: an executable relation producer/consumer parity gate; automatic extractor fingerprints for cache validity; one canonical SCIP normalizer; richer receiver/type/signature metadata; modern JS/TS module/package resolution; canonical domain identities distinct from source occurrences; and portable semantic identity for federation. The composite matrix's Kùzu/LanceDB/memory/editing recommendations are **not** adopted as Blueprint requirements because they conflict with the current Blueprint boundary and with the measured adequacy of its SQLite/vector substrate.

The final Membrane subsystem deep-dive then resolves the remaining doctrine conflicts. **Project convention detection is promoted into committed Blueprint ownership once its weak-evidence contract is explicit.** Conversely, dense semantic retrieval/hybrid fusion return to exploratory status, and LSP is narrowed from an authoritative live source to an **on-demand verification/cross-check lane**. Similarity never becomes a resolution tier. The same pass also requires versioned declarative resolution rules where practical, deterministic emit-side semantic export, and bounded generation-history truth queries without adopting a separate bitemporal store.


## Final reconciliation note

The packet is now intentionally stricter than the donor syntheses in three places: (1) exact/source-backed resolution remains the only route to canonical identity edges; (2) optional LSP can verify or contradict a resolution but does not silently replace Blueprint truth; and (3) semantic vectors, if ever promoted, remain retrieval-only weak candidate discovery rather than a resolution tier or correctness dependency.

## Revision 2026-09-05 — convergence hole

Review against the running Blueprint daemon found one hole in the packet. The packet counted
`BPT-021` and `BPT-043` among its delivered atoms, so Gate A could have been declared passed while
`membrane_context` still returned nothing. Three changes close it:

1. **`BPT-021` and `BPT-043` reclassified to partial** (`03`, §1.1), with the daemon receipt as evidence.
   Delivered/partial/missing is now 46/20/2. **`BPT-019` remains delivered** — honest freshness is behaving
   correctly by refusing to claim `current`; the defect is upstream convergence, not the freshness contract.
2. **INV-024 added** (`01`): every automatically-marked pending domain must have an automatic clear path.
   Manual-only completion is a canon violation, asserted in CI rather than left to documentation discipline.
3. **BPQ repair extended past `advance applied clock`** (`02`) to complete pending domains, reseal and advance
   the generation — with the end-to-end acceptance test: edit Markdown, let the watcher converge, and
   `membrane_context` returns evidence with no manual `blueprint build` or `phase2 seal`. Gate A gains the
   matching criteria.

The mechanism, verified in source: `blueprint/src/graph/delta-store.mjs:392` marks the `doc` domain inside the
watcher on any `.md` change; the only clear is `blueprint/scripts/blueprint.mjs:3729` inside `phase2 seal`, a
manual `one_shot` lease. The words `phase2`, `seal`, `barrier` and `domainsPending` appeared nowhere in the
original packet, so nothing in it forced this fix.
