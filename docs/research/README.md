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
    (`adaptive_retrieval`, `agent_architectures`, `context_engineering`,
    `context_memory`, `evaluation`, `rag_core`, `security`, `surveys`).
  - `overlap/` — papers whose mechanisms also matter to Cortex
    (`graph_structured_retrieval`, `repository_context`); duplicated here
    deliberately so the two systems' corpora stay independent. This is the
    *only* intentional cross-folder PDF duplication in this corpus — do not
    read a paper appearing in both `core/` and `overlap/` as an unnoticed
    duplicate; it is deliberate.
  - `articles/` — the three web-article write-ups saved as markdown
    (Anthropic, Manus, Chroma context-rot), kept apart from PDFs since they
    have no arXiv id or byte-identity to check.
  - `MANIFEST.md` — inventory of every `core/` + `overlap/` PDF (48 files)
    with byte sizes and sha256 hashes, for integrity checking.

  There is no `reading/` junk-drawer any more: a 2026-08-18 pass checked
  every PDF for exact content duplication (sha256) and near-duplication
  (shared arXiv id) across the whole `papers/` tree and found **zero**
  duplicates — every paper that had accumulated in `reading/` was a genuinely
  distinct work, not a second copy of something already in `core/`/`overlap/`.
  The 16 loose PDFs were filed into the existing topic folders, or into the
  new `agent_architectures/` topic (self-improving/open-ended agent design —
  CodeAct, Darwin Gödel Machine, OMNI-EPIC, automated agentic-system design,
  multi-agent evolving orchestration, the observation-masking complexity-trap
  paper) since the existing taxonomy had no home for that theme. The 3 loose
  `.md` articles moved to `articles/`.

- **`notes/`** — kebab-cased reading notes distilling individual papers and
  articles (originally the `momo-research` log). `figures/` holds the
  associated images.

- **`artifacts/`** — the supplied Graph Memory source artifact: the original
  PDF plus a corrected semantic HTML extraction.

## Navigating

- Want the "why" behind a Membrane design decision? Start in `synthesis/`.
- Want to know how Membrane compares to another tool? Start in `competitors/`.
- Want the primary source for a technique? Search `papers/core/` or
  `papers/overlap/` by topic, or `papers/articles/` for the web write-ups;
  check `papers/MANIFEST.md` to confirm a file's integrity.
- Want a fast summary instead of a full paper? Check `notes/` first.

Fresh code and the implementation guide outrank anything in this corpus —
research here can be stale, superseded, or exploratory by nature.

## Supersession: the four consolidation passes

`synthesis/00-03`, `competitors/sources/*`, and root `docs/MEMBRANE-IMPLEMENTATION-GUIDE.md`
are four successive consolidation passes over largely the same underlying
research and repository evidence, each one superseding the last. They are
kept side by side deliberately (task history and rationale, not a spec), but
a reader should know the order and which one actually governs:

1. **`synthesis/00-MASTER-SYNTHESIS.md` → `01-MEMBRANE-GAP-ANALYSIS.md` →
   `02-MEMBRANE-IMPROVEMENTS.md` → `03-BUILD-PLAN.md`** (2026-07-26) — the
   first consolidation pass, over the academic paper corpus. Each document
   explicitly builds on the previous one in this chain (see their own
   cross-links and dated headers).
2. **`competitors/sources/MEMBRANE-ABSORPTION-LEDGER.md`** (2026-08-17) — a
   second, independent consolidation pass, this time over ~60 surveyed
   competitor repositories rather than papers; reconciles four raw
   competitor-analysis registers into one dependency-ordered absorption list.
3. **`competitors/sources/MEMBRANE-CANONICAL-MASTER-IMPROVEMENT-GUIDE.md`**
   (2026-08-18) — folds the absorption ledger and the paper-corpus synthesis
   together into one proposed canonical guide.
4. **`competitors/sources/MEMBRANE-IMPLEMENTATION-GUIDE.md`** (2026-08-17) —
   a sibling implementation-facing pass in the same folder; largely
   overlapping content with the canonical master guide above, written from
   the same inputs.
5. **`docs/MEMBRANE-IMPLEMENTATION-GUIDE.md`** (repo-root `docs/`, outside
   this directory) — the actual, current **implementation authority**. This
   is the one to implement against; everything above it in this list is
   provenance for how it was derived, and should be read as historical, not
   as a live spec. Where any research document here disagrees with the
   root-`docs/` guide or with current code, the root guide and the code win.
