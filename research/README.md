# Membrane Research Corpus

This directory is comparative and academic research that informed Membrane's
design. **It is not product documentation and not part of the shipped
product.** Read code and `README.md`/`docs/` at the repo root for what
Membrane actually does; the implementation authority derived from this
research lives at `docs/MEMBRANE-IMPLEMENTATION-GUIDE.md` (repo-root `docs/`,
outside this directory). Treat everything here as provenance — why a design
choice was made — not as a spec to implement against.

## Layout

- **`synthesis/`** — the working analysis that turned the raw research into
  a plan: `00-MASTER-SYNTHESIS.md`, `01-MEMBRANE-GAP-ANALYSIS.md`,
  `02-MEMBRANE-IMPROVEMENTS.md`, `03-BUILD-PLAN.md`, an `INDEX.md` of the
  paper library, and `NOTE-FOR-BLUEPRINT-AGENT.md`. Start here for "why."

- **`competitors/`** — comparative analysis of ~60 competing memory/context
  tools. `CORPUS-INDEX.md` lists the surveyed repositories with upstream
  links; `sources/` holds the three large consolidation documents (absorption
  ledger, canonical improvement guide, implementation guide) written from that
  comparative pass and later folded into the implementation authority.

- **`papers/`** — the academic paper library.
  - `core/` — papers selected primarily for Membrane, grouped by topic
    (`adaptive_retrieval`, `context_engineering`, `context_memory`,
    `evaluation`, `rag_core`, `security`, `surveys`).
  - `overlap/` — papers whose mechanisms also matter to Cortex
    (`graph_structured_retrieval`, `repository_context`); duplicated here
    deliberately so the two systems' corpora stay independent.
  - `reading/` — a looser set of papers and articles read during research
    but not sorted into the curated topic tree above.
  - `MANIFEST.md` — inventory of `core/` + `overlap/` PDFs with byte sizes
    and sha256 hashes, for integrity checking.

- **`notes/`** — kebab-cased reading notes distilling individual papers and
  articles (originally the `momo-research` log). `figures/` holds the
  associated images.

- **`artifacts/`** — the supplied Graph Memory source artifact: the original
  PDF plus a corrected semantic HTML extraction.

## Navigating

- Want the "why" behind a Membrane design decision? Start in `synthesis/`.
- Want to know how Membrane compares to another tool? Start in `competitors/`.
- Want the primary source for a technique? Search `papers/core/` or
  `papers/overlap/` by topic, then `papers/reading/` if not found there;
  check `papers/MANIFEST.md` to confirm a file's integrity.
- Want a fast summary instead of a full paper? Check `notes/` first.

Fresh code and the implementation guide outrank anything in this corpus —
research here can be stale, superseded, or exploratory by nature.
