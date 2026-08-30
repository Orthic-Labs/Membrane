# ledger-eval-v1 — Ledger held-out retrieval evaluation corpus

Status: **candidate evaluation corpus**, not yet run against any Ledger recall
implementation. This directory is data authoring only — it contains no
production code and changes nothing about how Ledger indexes, tokenizes, or
ranks anything today.

Built against `docs/architecture/subsystems/ledger.md`
section 12 ("Evaluation architecture") and section 18 ("Test architecture" —
`Quality` row: "real paired held-out corpus and confidence intervals").

## What this is

152 query/expected-answer cases over **real Markdown files already in this
repository** (`docs/`, subsystem canons, top-level `README.md`) — no
synthetic documents were authored for this corpus. Every case was built by
first reading the real source document, then writing a case whose `quote`
and/or `heading` field is a verbatim substring of that document. A build-time
validator (`build_corpus.py`, kept alongside the corpus — see "How to
extend" below) greps every case's `quote`/`heading` against the actual repo
files and refuses to emit the corpus if anything doesn't match byte-for-byte.
That validator is the corpus's own regression test: run it any time a
referenced document changes, and it will fail loudly instead of silently
drifting from the real files.

`manifest.json` lists the 29 distinct source documents the 152 cases
reference, each with a `sha256` captured at authoring time. **This corpus is
a frozen snapshot bound to those hashes.** If a referenced document's content
changes upstream, its cases may no longer describe that document accurately;
re-run `build_corpus.py` (it recomputes hashes from the live tree) and diff
`manifest.json` before trusting old cases against new content.

## Files

```
schema.json         JSON Schema for one case (one line of any cases/*.jsonl file)
manifest.json        corpus-level metadata: totals, per-category counts, and
                      the sha256 + byte size of every referenced source document
build_corpus.py       the authoring/validation script that produced everything
                      below from hand-curated case definitions; re-run it to
                      validate, regenerate, or extend the corpus
cases/train.jsonl     51 cases — authoring/example split
cases/dev.jsonl       51 cases — tuning split
cases/heldout.jsonl   50 cases — held-out split, use exactly once at activation
```

Each line of `cases/*.jsonl` is one JSON object conforming to `schema.json`.
Fields: `id`, `split`, `case_type`, `tags`, `source_corpus`, `query`,
`query_script`, `expected` (`status` / `match_mode` / `targets[]` /
`plausible_distractors[]`), `notes`. See `schema.json` for the full contract,
including what `status: "no_match"` and `status: "relocation"` mean.

## Train / dev / held-out methodology (canon section 12.1)

The canon's ordering is:

```
authoring/train split
        |
        v
development/tuning split
        |
        v
freeze candidate
        |
        v
held-out test split
```

`cases/train.jsonl` and `cases/dev.jsonl` are for tokenizer selection, BM25
field-weight tuning, query normalization, title-chain ablation, and any rank
tuning. **`cases/heldout.jsonl` must not be looked at, iterated against, or
used to pick a configuration.** It is promotion evidence, run exactly once
per frozen candidate (canon: "Do not tune on held-out" / "no tuning on final
held-out" is a hard acceptance gate). If a future change needs re-tuning,
that requires a **new** held-out set (or a documented, versioned exception),
not a second pass over this one.

Splitting is disjoint by case, not by document: a handful of documents (most
prominently the Ledger canon itself, and `docs/product/installation/roots.md`) are
referenced by cases in more than one split, because a real evaluation corpus
for a specific subsystem's canon is necessarily going to ask many independent
questions about that subsystem's own canonical document. No single document
supplies more than about a fifth of any split's cases; see the per-split
document-frequency table below. What is disjoint is the **case set** — no
query string repeats across splits, and the two CJK-scarce categories (see
"Known limitations") are the only place the same short snippet of source text
is queried from more than one split, deliberately worded differently each
time so a system can't "memorize" a literal dev-split query string and pass
the held-out case for free.

Top referenced documents per split (from `manifest.json` + `cases/*.jsonl`):

| Split | Top document | Cases from it | Share of split |
|---|---|---|---|
| train | Ledger canon | 11 | 22% |
| dev | Ledger canon | 12 | 24% |
| heldout | Ledger canon | 10 | 20% |

## Sample-size / statistical-power rationale (canon section 12.2)

The canon explicitly rejects an arbitrary "50–100 questions" corpus size
with no power reasoning behind it. This corpus does not declare a single
fixed minimum-detectable-effect (MDE) number instead, because Ledger has not
shipped a first candidate yet to size an MDE against — there is no existing
effect (e.g. "5-point Recall@5 lift") to power a sample size calculation for.
What it commits to instead, per the canon's explicit alternative ("or use
paired per-query comparisons with predeclared bootstrap confidence
intervals"), is the **evaluation method**:

- Every candidate ranking implementation (`legacy_scan`, a frozen
  `ledger_fts` candidate, and any later challenger) MUST be run over the
  *same* fixed query set (train for iteration sanity checks, dev for tuning,
  heldout for the one-shot promotion decision) — never a resampled or
  reshuffled query set per candidate.
- Because every candidate is run on the same queries, **paired** per-query
  deltas (e.g. `recall_at_5(new) - recall_at_5(old)` for each of the 50
  held-out queries) are the primary statistic, not two independent means.
- Confidence intervals on the paired deltas MUST be computed by bootstrap
  resampling of the *query set* (resample the 50/51 cases with replacement,
  recompute the mean paired delta, repeat >= 2000 times) and predeclared
  (interval width and resample count fixed before looking at held-out
  results), per canon section 19's "Measured gates": *"quality delta reported
  with predeclared paired statistics/intervals"*.
- 50 held-out cases (and 51 dev cases available to estimate expected effect
  sizes and interval widths before the held-out run) is a floor sized to give
  every one of the 17 coverage categories at least 2 held-out cases apiece
  (see the category table below) — not a number chosen to make a target
  p-value easy to hit. If a future promotion decision needs a tighter
  interval than 50 paired held-out observations can support for a specific
  category (CJK and stale-relocation are the categories most likely to need
  this — they have only 2 held-out cases each), the right fix is to extend
  that category's case count in a new corpus revision (see "How to extend"),
  not to shrink the interval's stated confidence level to fit the data on
  hand.
- The per-category and per-split counts in `manifest.json`'s
  `caseTypeCounts` are exactly what a harness needs to additionally report
  **per-category** paired deltas (e.g. "identifier queries improved, CJK
  queries did not"), which the canon's metrics section (12.4) requires
  alongside the aggregate.

## Coverage categories (152 cases, `case_type` field)

17 categories, each covering an explicit requirement from canon section 12.3
and the task brief. Counts are `train / dev / heldout`.

| `case_type` | train | dev | heldout | total | What it exercises |
|---|---|---|---|---|---|
| `exact_document` | 5 | 5 | 4 | 14 | Direct document-title lookup |
| `exact_section` | 4 | 4 | 4 | 12 | Direct heading/section lookup |
| `table_content` | 3 | 3 | 3 | 9 | Retrieval into a GFM table cell/row |
| `fenced_code` | 3 | 3 | 3 | 9 | Retrieval into a fenced code block (json/sh/text/mermaid) |
| `list_item` | 3 | 3 | 3 | 9 | Retrieval into a bulleted/numbered/checklist list item |
| `blockquote` | 3 | 3 | 3 | 9 | Retrieval into a `>` blockquote |
| `link_reference` | 3 | 3 | 3 | 9 | Retrieval of a Markdown link's target document/anchor |
| `negative_no_answer` | 3 | 3 | 3 | 9 | Plausible query with **no correct answer** in-corpus (false-positive control) |
| `paraphrase` | 3 | 3 | 3 | 9 | Same information need, low lexical overlap with the target text |
| `non_ascii_cjk` | 2 | 2 | 2 | 6 | CJK-only / non-ASCII query (current tokenizer drops these — canon L1) |
| `mixed_script` | 2 | 2 | 2 | 6 | Latin + non-Latin script mixed in one query |
| `identifier_snake_case` | 3 | 3 | 3 | 9 | `snake_case` developer-identifier query (canon 8.2) |
| `identifier_camel_case` | 3 | 3 | 3 | 9 | `camelCase`/`PascalCase` developer-identifier query |
| `identifier_path_fragment` | 3 | 3 | 3 | 9 | Source-path-fragment query (e.g. `src/guide/doc_spine.rs`) |
| `short_query` | 3 | 3 | 3 | 9 | 1–2 term query (canon 8.3 lane policy) |
| `multi_section_synthesis` | 3 | 3 | 3 | 9 | Requires combining 2+ sections/documents (`match_mode: all_of`) |
| `stale_relocation` | 2 | 2 | 2 | 6 | Retired/renamed/not-yet-landed target — refusal or relocation, not best-effort text |
| **Total** | **51** | **51** | **50** | **152** | |

Every case additionally carries free-form `tags` for secondary coverage (for
example, an environment-variable bullet list is tagged both `list_item` and
`identifier`), so a harness that wants a different cut of the same 152 cases
can group by `tags` instead of `case_type`.

## Two real corpora, different authoring styles (canon 12.3)

`source_corpus` on every case is one of:

- **`membrane-docs`** (127 cases, 26 documents) — this repository's own
  hand-written subsystem canons, generated product-truth docs, workflow
  prompts, and top-level READMEs. Terse, invariant-heavy, heavily
  cross-referenced prose; GFM headings are used consistently and mean what
  they say.
- **`research-papers`** (25 cases, 3 documents) — `docs/research/legacy-source-corpus/papers/**`,
  PDF-extracted academic paper text (`converted_by: pymupdf get_text` in
  each file's YAML frontmatter). This is genuinely different authoring
  structure, not a relabeled copy of the same style: citation-dense prose,
  author-name Unicode noise, and in two of the three referenced files **no
  real GFM heading structure at all** below the frontmatter title (one file's
  only `#`/`##` lines are Python code comments from an appendix code
  listing, not section headings). Cases against these documents mostly set
  `heading: null` for exactly that reason — see `schema.json`'s note on
  `heading` being null "when the document has no GFM heading structure at
  all." This is the corpus's honest way of satisfying "a second real corpus
  with different authoring style/structure" without inventing content.

## Known limitations (be explicit about what was hard to populate)

- **`non_ascii_cjk` and `mixed_script` (6 cases each, the two smallest
  categories).** This repository's real Markdown is overwhelmingly English
  prose. A full-repo scan (`docs/**/*.md`) found genuine CJK-script text in
  exactly **two** files: one ICLR-paper appendix containing a verbatim
  Japanese MGSM benchmark question, and one scraped blog article whose
  language-switcher boilerplate lists CJK/Thai/Arabic language names. Every
  `non_ascii_cjk` and `mixed_script` case is built from one of those same two
  documents. Different splits deliberately use different exact query
  substrings from the same source text (different clauses of the Japanese
  sentence, different language-name tokens) so no literal query string
  repeats across splits, but a harness should not read strong statistical
  power into 2-per-split CJK results — this category is a correctness smoke
  test (does a CJK-only query still retrieve *something*, per canon L1),
  not yet a well-powered quality benchmark. Extending this category to a
  larger, more diverse CJK sample is the single highest-value follow-up to
  this corpus (see "How to extend").
- **`stale_relocation` (6 cases, also small).** Genuine stale/retired/
  not-yet-landed content that is *provably* absent or *provably* unchecked
  (rather than merely asserted by the case author) is naturally rare in a
  repository that is being actively kept consistent. The 6 cases here are
  real, verified instances: a retired file (`docs/subsystems/spine.md`, gone
  from the tree, its retirement recorded in `ledger.md`'s own Definition of
  Done), a superseded planning doc (`04-GUIDE-MARKDOWN-INDEXING-REVIEW.md`,
  also gone, superseded-by recorded in the canon's own header), an unchecked
  Definition-of-Done checkbox, a documented-but-not-yet-executed module
  rename, and the live `Guide` -> `Ledger` naming-migration window itself
  (agent-rules.md: *"Guide is retired; `guide`-named code/paths are pending
  rename, not a second name"* — yet `README.md` and `docs/product/README.md` both
  still say `Guide` in their six-axes tables at the time of this snapshot).
  That last pair of cases is intentionally reused for a `multi_section_synthesis`
  case too (`LEDG-EVAL-0140`) because it is simultaneously the corpus's
  clearest naming-drift example and its clearest cross-document synthesis
  example.
- **`negative_no_answer` false-positive strength.** Cases were chosen to be
  *domain-plausible* (e.g. "Ledger's default BM25 k1/b values", not "what is
  the capital of France") specifically so a lexical matcher has something
  vocabulary-adjacent to over-rank; `plausible_distractors` on those cases
  names the document a naive matcher is likeliest to wrongly surface. Three
  of the nine also cite the exact canon passage that makes the negative
  subtle: the query about a "fixed minimum held-out count" targets a section
  (12.2) that is topically dead-on but explicitly declines to state a
  number — testing whether a system conflates "relevant" with "answers the
  question."
- **One authoring wrinkle worth knowing about, not a limitation of the
  corpus:** `docs/research/legacy-source-corpus/papers/darwin-godel-machine_2505.22954.md`
  contains a literal embedded NUL byte (confirmed via direct byte
  inspection) yet decodes as valid UTF-8 and reads normally. It was kept
  as-is rather than "cleaned" because it is real content already in the
  repository and a real edge case a production Markdown parser will
  eventually meet.

## How to extend

1. Add new case definitions to `build_corpus.py` (`add(split, case_type,
   query, targets, ...)` calls), sourcing every `quote`/`heading` from a
   document you have actually read in the live tree — do not paraphrase from
   memory.
2. Run it: `python3 build_corpus.py --repo-root <repo-root> --out-dir <tmp-dir>`.
   It greps every `quote` and `heading` against the real files and refuses to
   emit anything if a match fails, so a typo or a stale quote fails loudly
   instead of silently shipping a wrong ground-truth label.
3. Copy the regenerated `cases/*.jsonl` and `manifest.json` over the ones in
   this directory. Diff `manifest.json`'s `sha256` values against the
   previous version — if a referenced document's hash changed for a reason
   unrelated to your addition, some existing case may now be stale and needs
   review, not silent regeneration.
4. **Do not add new cases to `cases/heldout.jsonl` after it has been used for
   a promotion decision.** A held-out set that grows after being "peeked at"
   is no longer held out. Either extend `train`/`dev` only, or mint a new
   versioned corpus directory (`ledger-eval-v2/`) that supersedes this one
   for held-out purposes while this one's held-out results remain a valid
   historical record.
5. Keep `schema.json` and this README in sync with any field changes to the
   case shape.

## What this corpus is not

- Not synthetic-fixture mechanics evidence. Per canon section 12.3 ("Synthetic
  fixtures remain mechanics tests, not promotion evidence"), the sibling
  fixture `../doc-spine-shadow-replay-v1/` is exactly that kind of synthetic
  mechanics test (three tiny invented documents, used to exercise shadow-replay
  scoring/reporting code paths) — it is unrelated to this corpus and neither
  directory should be read as validating the other's claims.
- Not yet run against anything. No `legacy_scan` or `ledger_fts` baseline
  numbers exist for this corpus as of authoring time. A future harness
  consuming this corpus is what turns it into promotion evidence; until then
  it is only the frozen query/answer set that harness will run against.

## Running the promotion harness

`tests/ledger_eval_v1_harness.rs` (sibling of this fixture directory, one
level up) indexes this corpus's real source documents through the production
Ledger sync/index path (`ledger::doc_spine::sync`) and evaluates every case
in `cases/dev.jsonl` and `cases/heldout.jsonl` against both recall arms
(`legacy_scan`, `ledger_fts`) via `ledger::doc_spine::recall_shadow`, which
always computes both lanes regardless of the persisted activation mode.

```
cargo test --manifest-path engine/Cargo.toml -p membrane-runtime \
  --test ledger_eval_v1_harness -- --nocapture
```

(Prefer the workspace RightKit cargo shim per `docs/agent-rules.md` when
available; the command above is the plain-`cargo` fallback.) Two tests run:
`ledger_eval_v1_dev_split_both_arms` (repeatable, for tuning/reporting) and
`ledger_eval_v1_heldout_split_both_arms` (run exactly once per frozen
candidate per canon section 12.1 — do not iterate against its output). Each
prints a `corpus_digest=` line binding the run to the exact manifest/case
files evaluated, per-arm Recall@1/@5 and MRR, a per-`case_type` breakdown,
and — for the held-out test — the frozen promotion decision rule's verdict.
