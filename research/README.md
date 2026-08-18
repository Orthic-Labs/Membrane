# Cortex Research

This directory is **research input only**. Nothing here is product documentation
or implementation authority — see `docs/architecture.md` and `README.md` at the
repo root for what Cortex actually does. This corpus is Cortex-only; it is not a
shared paper pool with Membrane.

## Layout

- `papers/` — academic papers on graph/knowledge-graph retrieval, repository-level
  code retrieval, program analysis, and incremental computation. See
  `papers/INDEX.md` for the full annotated catalog.
  - `papers/core/` — papers selected primarily for Cortex.
  - `papers/overlap/` — papers whose mechanisms also matter to Membrane;
    physically duplicated here so overlap doesn't force the two systems into
    one corpus.
- `competitors/` — competitor corpus index (`CORPUS-INDEX.md`); the underlying
  cloned repos are gitignored and not vendored in this repository.
- `source-artifacts/` — raw captured source material (e.g. the supplied Graph
  Memory artifact and its corrected semantic HTML extraction).
- `RESEARCH_MANIFEST.md` — file-level manifest (sizes, hashes) for the papers
  and source artifacts in this directory.

## Coverage

Core:
- graph / knowledge-graph retrieval
- repository-level code retrieval
- program analysis / code property graphs
- incremental computation / invalidation

Overlap included here because it affects Cortex retrieval quality or evidence safety:
- adaptive and value-based retrieval
- corrective/self-reflective retrieval as a benchmark lens
- poisoning / adversarial retrieval
- retrieval evaluation and traceback
- RAG + reasoning synthesis

The overlap copies are intentional. Cortex and Membrane remain separate systems.

PDF count in this Cortex directory: **26**.
