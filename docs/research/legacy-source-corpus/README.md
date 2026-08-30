# Membrane Research Corpus

This directory is comparative and academic research that informed Membrane's
design. **It is not product documentation and not part of the shipped
product.** Read code, `README.md`, & `docs/architecture/` for what Membrane
actually does; current capability state lives in `docs/canon/` & open work in
`docs/pending/README.md`. Treat everything here as provenance — why a design
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

- **`papers/`** — the paper library, 48 papers stored as **extracted markdown,
  not PDFs**. Flat: topic is legible from the filename and indexed by
  `MANIFEST.md`. Each file's frontmatter carries `topic`, `source` (arXiv or
  publisher URL) and `source_pdf_sha256`, so the exact original is re-fetchable
  and verifiable. Figures, equations and tables do not survive extraction —
  fetch the PDF when they matter.
  - `articles/` — three web-article write-ups (Anthropic, Manus, Chroma
    context-rot) kept apart from the papers.
  - `MANIFEST.md` — inventory grouped by topic, with source links and the
    original PDF's size and hash.

- **`notes/`** — kebab-cased reading notes distilling individual papers and
  articles (originally the `momo-research` log). `figures/` holds the
  associated images.

- **`artifacts/`** — the supplied Graph Memory source artifact: the original
  PDF plus a corrected semantic HTML extraction.

- **`derived-architecture/`** — non-authoritative historical system/subsystem
  views retained because Ledger evaluation cases exercise their exact prose.

## Navigating

- Want the "why" behind a Membrane design decision? Start in `synthesis/`.
- Want to know how Membrane compares to another tool? Start in `competitors/`.
- Want the primary source for a technique? Grep `papers/*.md` directly — the
  text is searchable now — or scan `papers/MANIFEST.md`, which groups every
  paper by topic with its source link.
- Want a fast summary instead of a full paper? Check `notes/` first.

Fresh code and the implementation guide outrank anything in this corpus —
research here can be stale, superseded, or exploratory by nature.

## Supersession: the four consolidation passes

`synthesis/00-03` & `competitors/sources/*`
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
5. **`docs/architecture/` + `docs/canon/` + `docs/pending/README.md`** — current
   architecture, capability state, & open-work authorities. These are what to
   implement against; everything above them in this list is
   provenance for how it was derived, and should be read as historical, not
   as a live spec. Where any research document here disagrees with the
   current authorities or code, current authorities & code win.
