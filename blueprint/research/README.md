# Blueprint Research

This directory is **research input only**. Nothing here is product documentation
or implementation authority — see `docs/architecture.md` and the repo-root
`README.md` for what Blueprint actually does. This corpus is Blueprint-only; it is not
a shared paper pool with Membrane.

## Layout

- `papers/` — academic papers on graph/knowledge-graph retrieval, repository-level
  code retrieval, program analysis, incremental computation, and the retrieval /
  evaluation / adversarial work that bears on retrieval quality and evidence
  safety. Flat directory, one file per paper. See `papers/INDEX.md` for the
  catalog.
- `competitors/` — competitor corpus index (`CORPUS-INDEX.md`); the underlying
  cloned repos are gitignored and not vendored in this repository.
- `source-artifacts/` — raw captured source material.
- `RESEARCH_MANIFEST.md` — file-level manifest (sizes, hashes).

## Format

Papers are stored as markdown converted from their source PDFs with
`pymupdf4llm`. Conversion is lossy for equations and figures; each file carries
a header linking the canonical arXiv source, which is authoritative. Filenames
are `<arxiv-id>_Title.md`.
