# Ledger Markdown Indexing and Document Navigation Canon

**Date:** 2026-08-25  
**Status:** implementation-ready architecture; intended to become canonical Ledger implementation authority after adoption  
**Supersedes:** `04-GUIDE-MARKDOWN-INDEXING-REVIEW.md` in full  
**Canonical subsystem rename:** **Guide → Ledger**  
**Historical names:** Spine / Markdown Doc Spine / Guide  
**Parent system:** Membrane  
**Scope:** Markdown/document registration, source-bound projection, indexing, recall, exact resolution, generated virtual documents, migration, qualification, rollout, and rollback

## Executive decision

The Membrane document-navigation subsystem is renamed **Ledger**.

This is a complete product/architecture rename, not a nickname and not a new seventh subsystem.

The six Membrane axes become:

1. Pull
2. Push
3. Cortex
4. Blueprint
5. **Ledger**
6. Adapt

Historical `Guide` and `Spine` terminology is retired from current-product code, docs, generated truth, CLI names, status/read-model fields, tests, and operator surfaces after the bounded compatibility window described below.

Ledger answers:

> **Where in the registered document corpus is the relevant material, and can the exact current source bytes be resolved safely?**

Ledger is the canonical **document registry, Markdown structural index, retrieval, and source-resolution subsystem**.

It registers all eligible Markdown/document sources under the active grants, tracks their identities/revisions/hashes, projects them into document/section/block structure, indexes those projections, and returns source-bound navigation candidates.

Ledger does **not** become repository truth, durable learned knowledge, or the final context planner.

The source document bytes remain authoritative at their canonical source. Ledger's indexes and optional cached bytes are rebuildable projections.


## Runtime lifecycle binding (normative)

These decisions are canonical and take precedence over any wording later in this
document that implies a different runtime topology:

- Membrane runtime exists only inside the headless child daemon of the visible
  native tray, with OS-enforced lifetime coupling. There is no standalone or
  orphanable Membrane runtime.
- There is **no embedded CodeRight Membrane backend**. CodeRight binds to
  Membrane through Hub, or it has no binding.
- MCP and CLI surfaces are **stateless daemon clients/transports**. They never
  launch, auto-start, or register a Membrane process.
- **Tray off → no Membrane context.** Requests return typed
  `membrane_unavailable { reason: hub_inactive, retryable: true }`.
- **Ledger** is the canonical subsystem name; it replaces Guide.
- Blueprint is **independently usable but not independently resident**.
  Continuous watcher/freshness runs only inside the tray-owned daemon; with tray off, Blueprint
  access is an explicit bounded one-shot operation that never daemonizes.

---

# 1. Supersession of the previous Guide plan

This document supersedes the uploaded `04-GUIDE-MARKDOWN-INDEXING-REVIEW.md`.

The prior document's core technical diagnosis is retained:

- whole-document scan is inadequate;
- the shipped FTS experiment is not production recall;
- the ASCII-only query tokenizer is a live correctness defect;
- AST/source-position projection is the correct structural basis;
- Ledger-local FTS5 is appropriate;
- source hashes/revisions must fail stale;
- relation expansion must remain document-structural and bounded;
- evaluation must precede activation;
- Cortex/Blueprint storage must remain separate.

The prior naming decision is reversed.

Where it said **keep Guide**, this document requires **full Guide → Ledger rename**.

Where it used `guide::*`, `GuideDb`, `guide_doc_*`, `guide-index.sqlite3`, or `membrane guide`, the target names are `ledger::*`, `LedgerDb`, `ledger_doc_*`, `ledger-index.sqlite3`, and `membrane ledger`.

The previous `guide::ledger` generated-session module creates a naming collision under the new subsystem name and MUST be renamed as part of the migration.

---

# 2. Naming and migration contract

## 2.1 Canonical name

**Ledger** is the only current-product subsystem name after cutover.

Use:

- `Ledger`
- `ledger`
- `membrane ledger`
- `membrane_runtime::ledger`
- `LedgerDb`
- `ledger-index.sqlite3`

Do not use `Guide` or `Spine` in current architecture except in:

- historical documents;
- migration compatibility tests;
- explicit rename ledgers;
- one-release upgrade/error messages.

## 2.2 Existing session-ledger collision

Current code contains a generated per-session document under `guide::ledger`.

After the subsystem rename, do not create `ledger::ledger`.

Rename that mechanism to:

```text
ledger::session_projection
```

Recommended public/internal type migration:

```text
SessionLedgerInputV1        -> SessionDocumentProjectionInputV1
SessionLedgerDocumentV1     -> SessionDocumentProjectionV1
LedgerSourceCursor          -> SessionProjectionSourceCursor
LedgerEventV1               -> SessionProjectionEventV1
LedgerTaskV1                -> SessionProjectionTaskV1
LedgerArtifactV1            -> SessionProjectionArtifactV1
LedgerDecisionV1            -> SessionProjectionDecisionV1
build_session_ledger        -> build_session_projection
index_session_ledger        -> index_session_projection
```

If wire/storage compatibility requires retaining old serialized tags for one migration window, preserve them through explicit versioned conversion code. Do not keep the old names as parallel canonical APIs.

The generated human-readable artifact may still be described in UI copy as a **session ledger** if that phrase is useful, but the code owner is Ledger and the module/API name should avoid `ledger::ledger`.

## 2.3 Rename surface

The full rename MUST cover at minimum:

- canonical Membrane doctrine;
- subsystem map/system docs;
- `docs/subsystems/guide.md` → `docs/subsystems/ledger.md`;
- generated product truth and its source generator;
- Adapt references;
- Pull/Push/Cortex/Blueprint cross-references;
- native migration documents;
- Hub six-subsystem read model;
- Tauri/CLI status labels;
- internal subsystem enums;
- telemetry/diagnostic subsystem labels;
- CLI `membrane guide *` → `membrane ledger *`;
- Rust module/directory `membrane_runtime::guide` → `membrane_runtime::ledger`;
- `GuideDb` and Guide-owned table/file names;
- fixtures, benchmarks, golden files, schemas, and docs;
- CodeRight Membrane capability names;
- MCP/host-facing names if exposed;
- installer/support diagnostics.

Generated files MUST be regenerated from their owning sources; do not hand-edit generated truth.

## 2.4 Compatibility policy

This is a rename, not a permanent dual identity.

During upgrade only:

- old persisted Guide index state may be recognized and migrated/rebuilt;
- old CLI invocations may return a typed `renamed_to_ledger` diagnostic for one release if required;
- old configuration keys may be read only through an explicit migration;
- old Hub/read-model fields may be decoded only at the compatibility boundary if installed-version interoperability requires it.

After the compatibility window:

- no new current-product output says Guide;
- no runtime path prefers or emits Guide;
- no new database/table/file is created under Guide naming;
- no current docs teach Guide.

---

# 3. Canonical ownership

## 3.1 Ledger owns

Ledger owns:

- eligible document-source registration;
- canonical Ledger document identity metadata;
- source path/reference, revision and content-hash tracking;
- Markdown/GFM parsing for document navigation;
- source-positioned document/section/block projections;
- stable source-bound node identities;
- exact anchor/alias/relocation records;
- internal Markdown link/reference resolution;
- Ledger-local lexical retrieval;
- document structural traversal;
- generated virtual-document projection after source/lifecycle qualification;
- stale-source refusal and typed relocation;
- incremental sync and tombstones;
- complete transactional index generations;
- Ledger-owned rebuildable SQLite state;
- Ledger retrieval/resolution metrics and receipts.

## 3.2 Ledger does not own

Ledger does not own:

- repository/code truth — Blueprint;
- durable learned knowledge, memories, Taste, or Insights — Cortex after admission;
- behavioral learning — Adapt;
- final evidence fusion/admission/attention policy — Pull/Membrane planner;
- faithful reduction/compression — Push;
- CodeRight execution traces or generic eval datasets;
- source document authority when the canonical bytes live in Git/filesystem/external source;
- generic semantic relation graphs;
- LLM-authored indexing authority.

## 3.3 "Ledger holds all Markdown" — precise meaning

The intended product statement is:

> **Ledger holds the canonical registry and searchable structural projection of all eligible Markdown/document sources under its grants.**

This means Ledger knows:

- every registered document;
- where its authoritative source lives;
- its revision/hash;
- its structural nodes;
- its index entries;
- its links/aliases;
- how to resolve exact current bytes.

It does **not** require Ledger to become the authoritative owner of the source file bytes.

Optional raw-text caching is permitted as a rebuildable performance optimization when:

- the cached bytes are hash-bound to the authoritative source;
- cache invalidation is deterministic;
- stale bytes can never be silently served;
- erasure removes the cache;
- a rebuild can reconstruct Ledger from source.

Generated virtual documents are different because they may not have a filesystem source. They require a typed virtual-source authority/lifecycle contract before becoming recallable.

---

# 4. Locked invariants

1. Raw source bytes plus canonical source identity/revision are authoritative; Ledger projections are rebuildable.
2. Ledger returns navigation/index candidates and exact source resolution, never repository truth or durable-memory authority.
3. Scope/path/lifecycle/trust/influence/sensitivity eligibility runs before ranking.
4. Pull retains final authority/freshness/sufficiency/fusion/admission/budget/receipt policy.
5. Every returned node binds source range, span hash, source revision, Ledger generation, parser version, index schema version, and tokenizer/query-normalization version.
6. Hash/revision mismatch never returns silently changed text.
7. Parent/sibling/link expansion traverses source-derived document structure only.
8. Expansion is bounded by explicit `max_hops`, `max_nodes`, `max_edges`, cycle detection, and abstention.
9. Readers pin one published complete Ledger generation.
10. Mixed-generation artifact/node/link/FTS rows are never observable.
11. Ledger FTS lives in Ledger-owned storage and never opens Cortex storage.
12. Ledger never opens Blueprint SQLite directly.
13. Ledger has no vector/embedding store in this plan.
14. Semantic document retrieval is a future architecture decision triggered only by measured lexical/structural deficiency.
15. Unknown parser/schema/tokenizer versions degrade or rebuild explicitly.
16. Corrupt Ledger state rebuilds or fails closed.
17. Generated virtual session projections are non-recallable until their source/lifecycle contract and real consumer are qualified.
18. LLM output cannot manufacture source identity, document truth, links, or node authority.
19. An index existing on disk is not evidence it is shipped.
20. A capability is not landed until production-path reachability and acceptance evidence prove it.

---

# 5. Verified current-state diagnosis

The current implementation has useful components, but production recall remains materially shallower than the target.

Verified strengths include:

- Comrak/GFM parsing;
- source positions;
- headings/breadcrumbs;
- content and span hashes;
- hash-checked section reads;
- separate rebuildable SQLite state;
- lifecycle and sensitivity filtering;
- document projection code;
- tombstones/incremental metadata;
- deterministic stale-source refusal;
- generated session-document projection code.

Verified current weaknesses include:

- shipped sync writes one broad lexical document projection instead of wiring full AST/block projection;
- shipped recall scans all eligible whole-document lexical rows and scores them in Rust;
- production recall does not execute FTS `MATCH`/BM25;
- query tokenization is ASCII-alphanumeric only;
- a CJK-only query produces no terms and no results;
- node identity is too dependent on heading slug/ordinal;
- duplicate/renamed headings have weak relocation semantics;
- non-ASCII heading alias behavior is unsafe;
- durable typed nodes for lists/tables/blockquotes/HTML/references/longer fences are incomplete;
- persisted link/reference graph is absent;
- query-side identifier normalization is inadequate;
- quality tests emphasize mechanics rather than held-out retrieval outcomes;
- synthetic shadow replay is not operational activation evidence;
- generated session projection is not a first-class recallable virtual source.

One important landed behavior must be preserved: lifecycle and sensitivity eligibility already occur before scoring on the current SQL path.

---

# 6. Selected target architecture

```text
eligible document source bytes
        |
        v
canonical source identity + revision
        |
        v
Comrak/GFM parse with source positions
        |
        v
document / section / typed-block tree
        |
        +--> exact anchor / alias / relocation
        +--> Ledger-local FTS5/BM25
        +--> source-derived link graph
        |
        v
query normalization + lane retrieval
        |
        v
bounded structural/link expansion
        |
        v
Ledger candidates
        |
        v
Pull hard policy + fusion + admission + budget
        |
        v
hash-verified Ledger resolve
        |
        v
Push representation if selected
```

Ledger owns its candidate ranks and provenance. Pull decides cross-provider fusion and final attention.

---

# 7. Projection model

## 7.1 Parse once, preserve source positions

Parse Markdown once per source revision into a source-positioned AST.

Required block coverage includes:

- document root;
- headings;
- paragraphs;
- fenced code;
- indented code;
- lists/list items;
- blockquotes;
- GFM tables;
- HTML blocks;
- thematic breaks where useful;
- inline/reference links;
- link definitions;
- footnotes where supported;
- nested containers.

Do not use arbitrary character chunks as the primary structural model.

## 7.2 Persisted node contract

Persist per node/block:

```text
doc_id
node_id
parent_id?
ordinal
node_kind
heading_path
source_range
span_hash
searchable_text
link_targets[]
parser_version
projection_schema_version
fts_schema_version
tokenizer_id
query_normalizer_version
source_revision
ledger_generation
```

Optional:

```text
cached_raw_text
human_anchor_aliases[]
prior_node_ids[]
relocation_reason?
```

## 7.3 Identity

`doc_id` binds the canonical source identity, not merely the content hash.

`node_id` should be derived from stable structural evidence such as:

- document identity;
- parent identity;
- node kind;
- structural fingerprint;
- source span hash;
- bounded ordinal context.

Heading slugs are human aliases, not canonical identity.

Rename/move/duplicate insertion should produce explicit relocation/alias history rather than silently changing identity.

## 7.4 Span integrity

```text
span_hash = sha256(source_bytes[source_range])
```

Resolution verifies the current source bytes against the expected revision/hash before serving.

If mismatch:

- exact relocation if qualified;
- otherwise typed `stale`;
- otherwise typed `missing`.

Never serve best-effort changed bytes as though they matched the retrieved node.

---

# 8. Query processing contract

The previous plan was index-heavy and under-specified query handling. This section is mandatory.

## 8.1 Unicode normalization

Queries MUST not be ASCII-only.

Pre-register and test the normalization policy for:

- Unicode normalization;
- case handling where language-appropriate;
- combining marks;
- punctuation;
- path separators;
- Markdown punctuation;
- CJK text;
- mixed-script queries.

Do not silently convert a non-empty user query into zero retrieval terms.

## 8.2 Developer-identifier expansion

Developer-document retrieval must recognize identifiers.

For a query token such as:

```text
doc_spine
LedgerDb
membrane-runtime
ledger_doc_fts5
foo.bar.baz
src/ledger/doc_spine.rs
HTTPServerV2
```

generate a deterministic query representation that preserves the original term and may additionally expose components such as:

```text
doc_spine -> doc_spine, doc, spine
LedgerDb -> LedgerDb, ledger, db
membrane-runtime -> membrane-runtime, membrane, runtime
foo.bar.baz -> foo.bar.baz, foo, bar, baz
```

Rules MUST be deterministic and benchmarked.

Do not explode identifiers so aggressively that precision collapses.

## 8.3 Short queries

One- and two-token queries need an explicit lane policy.

Prefer:

1. exact path/title/anchor/identifier matches;
2. lexical FTS;
3. structural/link expansion from strong seeds.

Do not treat short queries as an excuse for broad full-corpus traversal.

## 8.4 Query operator safety

User text must not become raw FTS syntax.

Implement one safe query builder that:

- quotes/escapes terms;
- controls AND/OR policy;
- prevents column/operator injection;
- records the normalized query and lane behavior in debug/qualification receipts.

---

# 9. Ledger-local FTS

## 9.1 Storage

Use Ledger-owned FTS5 tables.

Recommended weighted fields:

```text
path
title
heading
body
identifier_aliases
```

No Cortex table reuse and no `cortex-store` dependency.

## 9.2 Tokenizer selection

Do **not** freeze the tokenizer before measuring it.

Evaluation procedure:

1. define a development split;
2. compare at least the relevant Unicode-capable FTS tokenizer options and trigram behavior;
3. include CJK-only, mixed CJK/Latin, identifiers, paths, short terms, and prose;
4. tune tokenizer and BM25 field weights on the dev split only;
5. freeze the winner;
6. run held-out promotion exactly once per frozen candidate.

## 9.3 BM25 weights

BM25 field weights are tunable parameters, not doctrine.

Tune only on the dev set.

Publish the frozen configuration alongside:

- schema version;
- tokenizer id;
- query-normalizer version;
- corpus version;
- benchmark receipt.

## 9.4 Production reachability

Activation evidence MUST demonstrate that production recall executes FTS `MATCH`/ranking.

An FTS table existing on disk, or a test proving results are unchanged when it is present, does not count.

Required proof includes at least one:

- instrumentation counter;
- trace/span;
- deterministic debug receipt;
- integration assertion bound to the production recall call path.

---

# 10. Structural and link retrieval

## 10.1 Section hierarchy

Persist the document hierarchy so strong hits can expose:

- parent section;
- child blocks;
- adjacent siblings;
- heading ancestry.

## 10.2 Link graph

Resolve source-derived Markdown links into typed navigation edges.

One canonical resolver must handle:

- inline links;
- reference links;
- autolinks where applicable;
- image references;
- relative file paths;
- fragments;
- Unicode anchors;
- broken targets.

Generator, validator, visualization, sync, recall, and resolve must share the same link-resolution semantics.

## 10.3 Expansion

Only expand from strong seeds.

Caps:

```text
max_hops
max_nodes
max_edges
```

must be frozen before activation and appear in receipts.

No generic relation-neighborhood traversal belongs in Ledger.

---

# 11. Title-chain contextualization is an experiment

Do not lock deterministic title-chain prefixes as an assumed retrieval improvement.

Candidate searchable augmentation:

```text
document title > H1 > H2 > H3
```

must be evaluated as a preregistered ablation:

- no prefix;
- deterministic breadcrumb prefix;
- optional alternative structural field weighting.

Measure:

- Recall@k;
- MRR;
- nDCG where appropriate;
- index bytes;
- query latency;
- context-token cost after resolution.

Only retain the prefix if it earns its cost.

This is not equivalent to LLM-generated contextual retrieval and must not be described as such.

---

# 12. Evaluation architecture

## 12.1 Do not tune on held-out

The previous plan's freeze order is corrected.

Use:

```text
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

Tokenizer choice, BM25 weights, query normalization, title-chain treatment, expansion parameters, and any rank tuning use the development split.

The final held-out set is not a tuning loop.

## 12.2 Corpus size and statistical gate

Do not assume 50–100 questions can support arbitrary small quality thresholds.

Before collecting the final corpus:

- state the minimum detectable effect or practical improvement needed;
- choose sample size accordingly;
- or use paired per-query comparisons with predeclared bootstrap confidence intervals.

Because every candidate can be run on the same query, paired evaluation SHOULD be the default where appropriate.

## 12.3 Corpus design

Use real document corpora.

Minimum query classes:

- exact document;
- exact heading;
- paraphrase;
- negative/no-answer;
- path/identifier;
- CJK/non-ASCII;
- mixed-script;
- table;
- list;
- fenced code;
- link/reference;
- stale/renamed/moved section;
- multi-section synthesis.

Use at least:

- real Membrane documentation;
- a second real corpus with different authoring style/structure.

Synthetic fixtures remain mechanics tests, not promotion evidence.

## 12.4 Metrics

Record:

- Recall@k;
- MRR;
- nDCG where ranking depth makes it meaningful;
- exact resolution rate;
- stale refusal correctness;
- relocation correctness;
- false-positive/no-answer behavior;
- p50/p95 cold and warm latency;
- index bytes;
- payload/WAL bytes;
- RSS delta;
- context-token cost for equivalent evidence;
- production-path lane execution counts.

---

# 13. Runtime contracts

| Operation | Input | Output / failure |
|---|---|---|
| **Sync** | source identity, revision, bytes, labels, grant, root | one verified atomic Ledger generation; typed omission/denial/rebuild failures |
| **Recall** | query, grant, eligibility, lane/budget | ranked source-bound candidates, normalized-query receipt, lane provenance, pinned generation |
| **Resolve** | doc/node id, expected revision/hash, pagination | exact bytes + metadata; typed stale/relocated/missing/ineligible |
| **Expansion** | strong seeds + caps | bounded source-derived parent/sibling/link nodes or typed abstention |
| **Pull consumption** | Ledger candidates + other providers | planner-owned fusion/admission/omission receipt |
| **Activation** | mode + qualification receipt | `legacy_scan`, `shadow`, or `ledger_fts` |
| **Migration** | prior Guide state/current sources | rebuild or typed conversion into Ledger generation |
| **Erasure** | granted document/source identity | delete Ledger-owned nodes/FTS/links/aliases/caches/exports |
| **Rebuild** | canonical source set + versions | deterministic equivalent Ledger projection |

---

# 14. Session document projection

The existing generated per-session "ledger" mechanism is not the Ledger subsystem itself.

Rename it to a session document projection.

## 14.1 Purpose

It creates a human-readable Markdown representation of selected session facts such as:

- recent events;
- tasks;
- artifacts;
- decisions;
- omissions;
- source cursor/hash.

It is useful for:

- human handoff;
- document-shaped navigation;
- cross-session orientation;
- export.

## 14.2 Authority

The projection is derived.

Its authoritative parents remain the underlying typed events/tasks/artifacts/decisions.

It MUST carry:

- source session identity;
- source cursor;
- source digest;
- derivation version;
- content hash;
- omission list;
- invalidation parent.

## 14.3 Recallability

It is non-recallable by default until:

1. a real consumer is defined;
2. a typed virtual-source authority exists;
3. lifecycle/invalidation is explicit;
4. privacy/retention policy is defined;
5. replay proves it adds value without duplicating stronger structured evidence.

Adapt SHOULD consume underlying structured events rather than this derived Markdown whenever both exist.

---

# 15. Cross-subsystem effects

## 15.1 Pull

Pull consumes Ledger candidate lanes and owns cross-provider fusion.

Ledger does not calibrate its BM25 score against Cortex vector similarity or Blueprint confidence.

A candidate fusion strategy such as Reciprocal Rank Fusion may be evaluated in Pull because it combines heterogeneous ranks without assuming comparable score scales.

## 15.2 Cortex

Cortex stores admitted durable knowledge and may retain Ledger source references.

Cortex does not copy the entire registered Markdown corpus merely because Ledger indexes it.

If a durable record cites a document section, re-resolution/re-anchoring should go through Ledger.

## 15.3 Blueprint

Blueprint remains repository/source truth owner.

Ledger consumes source identity/revision information but does not open Blueprint's database or validate code/document truth itself.

## 15.4 Push

Push may reduce Ledger-resolved blocks after Pull selects them.

Reduction must preserve:

- code fences;
- tables;
- links;
- protected spans;
- source order;
- raw recovery.

## 15.5 Adapt

Adapt may learn from retrieval outcomes and propose:

- alias additions;
- query-normalization changes;
- chunk/projection changes;
- ranking changes;
- failure detectors/evaluators.

Examples of useful structured outcome evidence:

- Ledger returned irrelevant sections repeatedly;
- Ledger omitted a section the agent later found manually;
- stale relocation failed;
- a CJK/identifier query returned zero despite an eligible source;
- Pull selected Ledger evidence that was repeatedly ignored;
- a ranking change reduced repeated manual searches.

Adapt cannot directly mutate Ledger index/ranking policy.

A proposed change must pass Ledger's own dev/held-out replay and promotion gate.

## 15.6 CodeRight

CodeRight should emit:

- query/task identity;
- Membrane context receipt;
- Ledger candidate ids/lanes used;
- resolver success/failure;
- subsequent manual searches;
- user correction;
- task/evaluator outcome.

That lets Adapt identify recurring document-retrieval failures and lets Ledger measure actual utility.

---

# 16. Production-path evidence invariant

Ledger inherits the Membrane-wide rule:

> **A capability is not landed until a test proves the production path executes it and frozen evidence shows it satisfies the acceptance baseline.**

For Ledger this specifically means:

- AST projections must be produced by shipped sync, not only helper tests;
- FTS must be queried by shipped recall;
- link edges must be used by shipped bounded expansion before "link navigation" is claimed;
- Unicode/CJK retrieval must work through the active production path;
- performance gates must run in qualification, not remain ignored;
- activation must have a receipt;
- rollback must actually select the old path;
- exact installed builds must exercise the same path where packaging matters.

---

# 17. Implementation sequence

## L0 — Rename authority first

Before broad retrieval work:

- approve Guide → Ledger as canonical;
- update Membrane doctrine/system map;
- update generated-truth sources;
- rename current-product docs;
- define the compatibility window;
- reserve target runtime/module/database names;
- rename the existing `guide::ledger` session module to `session_projection`.

Do not leave two current subsystem names while new implementation is landing.

## L1 — Fix live tokenizer correctness

Ship a bounded correction so non-ASCII-only queries cannot silently become zero terms.

Add tests for:

- CJK;
- accented text;
- mixed script;
- identifiers;
- short queries.

This fix is independent of the larger FTS architecture.

## L2 — Freeze evaluation methodology

Before tuning retrieval:

- define train/dev/test or authoring/dev/held-out corpus split;
- define MDE or paired-bootstrap gate;
- freeze metric definitions;
- freeze corpus inclusion policy;
- freeze no-answer cases;
- define performance hardware/method.

## L3 — Land shadow structural projection

- wire Comrak source-position AST projections into sync;
- persist typed nodes;
- persist links/aliases/relocation;
- publish complete transactional Ledger generations;
- keep production recall on legacy scan.

## L4 — Develop Ledger FTS on dev only

- implement Ledger-owned FTS;
- implement query normalizer;
- compare tokenizer options;
- tune field weights;
- run title-chain ablation;
- tune expansion caps if needed;
- freeze candidate.

## L5 — Shadow held-out replay

Run old vs frozen new candidate on the untouched held-out corpus.

Compare quality, latency, storage, stale refusal, no-answer behavior, and provenance.

## L6 — Activate `ledger_fts`

Activation requires a qualification receipt.

Prove active production recall executes FTS and structural resolution.

Keep atomic `legacy_scan` rollback for one release.

## L7 — Qualify structural/link expansion

Enable only after source-backed edges and caps pass held-out and operational tests.

## L8 — Normalize virtual session projection

Migrate the old session-ledger module/API to the session-projection identity.

Keep non-recallable until its own consumer contract qualifies.

## L9 — Retire legacy scan and Guide naming

After the rollback window and upgrade tests:

- remove `legacy_scan`;
- remove current-product Guide aliases;
- remove old Guide DB/table creation;
- delete/tombstone old Guide docs;
- ensure generated truth contains Ledger only.

---

# 18. Test architecture

| Layer | Required coverage |
|---|---|
| Parser | headings, duplicate/Unicode headings, tables, lists, blockquotes, HTML, references, indented code, longer fences, source ranges |
| Identity | rename, move, duplicate insertion, relocation, alias, exact span/hash |
| Storage | atomic publication, crash before publish, corruption, version mismatch, rebuild |
| Query | Unicode, CJK, mixed script, identifier splitting, short query, escaping |
| FTS | real production `MATCH`, field weights, ties, degradation |
| Resolver | exact bytes, stale refusal, relocation, pagination, deleted/ineligible source |
| Links | inline/reference/autolink/image/relative/fragments/Unicode/broken targets |
| Expansion | caps, cycles, abstention, provenance |
| Pull seam | lane ranks, eligibility unchanged, fusion owner, omissions |
| Push | block-type fidelity and raw restoration |
| Adapt seam | retrieval outcome observations and proposal-only improvement loop |
| Session projection | derived-source binding, invalidation, non-recallable default |
| Quality | real paired held-out corpus and confidence intervals |
| Operational | installed path, crash recovery, activation/rollback, no mixed generation |

---

# 19. Acceptance gates

## Hard gates

- Guide → Ledger rename complete on current-product surfaces;
- exact source resolution and stale refusal pass;
- CJK/non-ASCII-only queries do not silently become zero;
- identifier and short-query behavior is explicitly tested;
- production recall demonstrably uses Ledger FTS when active;
- readers see one complete Ledger generation;
- link resolution is deterministic;
- expansion is source-backed and bounded;
- full rebuild and randomized incremental rebuild are recall-equivalent;
- erasure removes all Ledger-owned payload/projections;
- Session projection remains non-recallable until separately qualified;
- no Ledger access to Cortex/Blueprint databases;
- no generic semantic graph/vector store sneaks in.

## Measured gates

- candidate is frozen before held-out;
- no tuning on final held-out;
- quality delta reported with predeclared paired statistics/intervals;
- latency/storage/RSS limits declared;
- equivalent evidence does not increase context-token cost without justified quality gain;
- production-path counters/receipts confirm the active capability;
- rollback is tested.

---

# 20. Rejected designs

Reject:

- Guide as a second current subsystem name after cutover;
- Guide-to-Ledger as a cosmetic docs-only rename;
- `ledger::ledger` naming collision;
- authoritative Ledger copies of repository Markdown that create dual-source truth;
- YAML/frontmatter as the primary indexing strategy;
- whole-file substring scans presented as finished search;
- fixed chunks that ignore Markdown structure;
- generated summaries as document truth;
- unbounded relation traversal;
- automatic LLM-extracted prose claims as truth;
- semantic-only retrieval;
- Ledger-owned vector store without a new measured architecture decision;
- Cortex FTS/schema reuse;
- direct Blueprint DB reads;
- LLM-generated indexing authority;
- tuning tokenizer/BM25/expansion on the held-out set;
- an unused index satisfying a performance claim.

---

# 21. Research basis and disposition

The previous review's research basis remains useful:

- GFM/CommonMark/AST source-position discipline;
- structure-aware Markdown chunking;
- document-structure-aware retrieval;
- Late Chunking as adjacent research;
- Anthropic Contextual Retrieval as an ablation reference, not a justification for LLM-authored Ledger index content;
- graph-memory-starter's build-time intelligence / deterministic traversal principle;
- OpenWiki's deterministic index/link hygiene;
- Omni's receipt/cache/recovery patterns where they belong in Push rather than Ledger.

Additional cross-cutting retrieval research belongs mainly in Pull/Push:

- Reciprocal Rank Fusion for heterogeneous rank lists;
- CRAG/Self-RAG for retrieval-quality assessment/corrective re-query patterns;
- Lost in the Middle for context ordering evaluation;
- LongLLMLingua for the principle that query-aware compression can outperform query-agnostic reduction.

Ledger should expose strong, source-bound candidate lanes and measurements. Pull and Push decide what to do with them.

---

# 22. Final canonical statement

> **Ledger is Membrane's document registry, Markdown structural indexing, navigation, retrieval, and exact source-resolution subsystem. It replaces Guide completely as the canonical subsystem name. Ledger registers all eligible document sources, maintains rebuildable source-bound AST/FTS/link projections, and returns exact hash/revision-bound navigation candidates. The authoritative source bytes remain with their canonical source; Blueprint owns repository truth, Cortex owns durable knowledge, Adapt owns behavioral learning, Pull owns final evidence admission/fusion, Push owns faithful reduction, and CodeRight owns agent execution.**
