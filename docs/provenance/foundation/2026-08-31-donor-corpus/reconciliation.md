# Donor corpus → Membrane canon reconciliation

## Scope & method

- Requested: discover new atomic capabilities only; no implementation comparison, ranking, porting, or code delivery.
- Target baseline: Membrane `3ba419002b28e82f8f45d1efd3a27b00d16e0d60`.
- Corpus: 27 repositories at exact commits in [`corpus.json`](corpus.json).
- Independent source-only inventories: [pass A](inventory-pass-a.md) (44 rows) & [pass B](inventory-pass-b.md) (61 rows).
- Original dual-pass evaluated 26/26 repositories & 105/105 inventory rows. A later [supplemental SOL review](openkb-sol-review.md) evaluated `VectifyAI/OpenKB` independently at its frozen commit.
- Evaluated: 27/27 repositories.
- Unresolved: 0.
- Excluded source classes: README/docs/marketing/issues, generated/build/cache/test-only evidence, benchmarks as capability proof, prior inventories, sibling outputs.

Both original inventories passed Foundation structural/source validation. Consolidation merged semantic synonyms without reopening source; disagreements were checked against smallest relevant donor, Membrane canon, & architecture surfaces. OpenKB is recorded as supplemental review, never as content covered by those original independent reports.

## New exploratory atoms

| ID | Owner | Atomic behavior | Why existing canon did not cover it | Donor evidence |
|---|---|---|---|---|
| `BPT-071` | Blueprint | Emit typed source-addressed evidence for explicit cross-language seams without semantic-similarity inference. | Existing Blueprint atoms cover language facts, providers, identities, traversal, & impact, but no atom promises explicit FFI/JNI/cgo/gRPC/PInvoke/WASM/COM bridge evidence. | `intuit__infigraph/crates/infigraph-core/src/bridges/mod.rs` at `1c3622f120edbbf120d1f89b6555626bb560d73d` |
| `LDG-027` | Ledger | Retrieve current sections through source-derived future-question aliases bound to exact current evidence; aliases remain non-authoritative & invalidate on drift. | `LDG-020`, `LDG-022`, & `LDG-024` cover projections, candidates, & identity aliases, not query-facing future-question metadata. Ledger owns this projection; Pull retains provider admission/fusion/final planning. | `graph-memory-starter/rag/distil.py` & `rag/build_index.py` at `496a6e9d9b9578943ec5ed34c2780de5a0fa5510` |
| `LDG-028` | Ledger | Normalize granted non-Markdown formats deterministically into hash-bound Markdown input with raw-source resolution, converter provenance, & typed loss accounting. | Existing Ledger starts from eligible Markdown; no atom owns governed pre-parser format normalization. Media ingestion remains excluded. | `VectifyAI__OpenKB/openkb/converter.py:141-236` at `ff54396e575ee6feb0113b631a34caa082b441cc` |

All three rows remain `EXPLORATORY`: Stage 2 records discoverable behavior without committing product delivery. Each has one implementation row, one qualification row, one decision, & stable provenance.

## Existing coverage

| Area | Consolidated donor mechanisms | Existing Membrane ownership |
|---|---|---|
| Pull | eligibility, scoped retrieval, hybrid/fallback/fusion, reranking failover, whole-evidence budgets, repository/document/memory context, source search | `PUL-001`–`PUL-033`, `CTX-011`–`CTX-014`, Blueprint Recall atoms |
| Blueprint | parsing, symbol/reference graphs, stable identity/provenance, incremental/branch/watcher refresh, staleness, search, impact, bounded traversal | `BPT-001`–`BPT-044` |
| Cortex | governed admission, temporal validity/conflict, lifecycle/retention/erase, episodic proposals, relation evidence, recall, receipts/events | `CTX-001`–`CTX-032` |
| Ledger | exact source rendering, Markdown sections/links, aliases, source-bound candidates | `LDG-002`–`LDG-006`, `LDG-014`–`LDG-024` |
| Adapt | evidence-bound rules, transcript handoff, reflection/proposal separation, background learning | `ADP-001`–`ADP-007`, `ADP-022`, `ADP-026`–`ADP-029`, `ADP-035` |
| Push | bounded whole-record packing, content-addressed recovery, reduction receipts | `PSH-005`, `PSH-006`, `PSH-011`, `PSH-019` plus Pull selection ownership |

## Explicit non-absorptions

| Donor mechanism | Disposition |
|---|---|
| Generic bidirectional multi-hop graph recall | Excluded: Blueprint already owns bounded semantic paths; Cortex deliberately excludes generic graph expansion. |
| General CodeQL/Joern taint/dataflow platform | Excluded by Blueprint architecture; typed callgraph behavior remains covered. |
| Generic memory graph CRUD/cube topology & named memory files | Implementation topology or conflict with canonical Cortex SQLite authority; not product atoms. |
| Transparent host prompt injection & agent-turn middleware | Host/CodeRight integration composite, not Membrane subsystem atom. |
| Index-derived query suggestions | Search UI/provider affordance, not task-context capability. |
| Generic receipt ledger under Ledger | Adapt/Cortex evidence concern, not document navigation/index state. |
| Entity/relation aliases as separate capability | Donor proves ungoverned entity seed aliases only. Query aliases are implementation input to `LDG-027`; no source-backed governed Cortex relation-alias atom was invented. |
| Model-tier search/reasoning displacement | Qualification candidate, not capability. No operative donor evaluator exists; future paired evaluation belongs CodeRight/qualification surfaces. |

## SOL message disposition

SOL's primary recommendation is retained as `LDG-027`, strengthened from donor whitespace-normalized quote containment to Membrane's exact section/revision/span-hash resolution. Its Cortex alias suggestion was not promoted into an atom because donor source lacks governance & measured Membrane relation-retrieval need. Existing RRF, bounded graph traversal, source-positioned chunks, planner budgeting, & fixed prompt injection were not duplicated.

## OpenKB & correction-pack disposition

OpenKB was absent from original frozen 26-repository corpus. Supplemental SOL source review found one new bounded behavior, recorded as `LDG-028`; all hashing, hierarchy, wikilink, rollback, watcher, raw-source, semantic compilation, & query-agent mechanisms map existing atoms or exclusions. See [OpenKB review](openkb-sol-review.md).

Supplied architecture-corrections archive was treated as evidence only. It refined `PUL-031`, `LDG-006`, `LDG-023`, `CTX-033`, `ADP-022`, & `ADP-036`; it created no new row. Two prior delivery claims were corrected to `PARTIAL`, making their repair work visible in generated pending truth. See [correction reconciliation](architecture-corrections-v2-reconciliation.md).

## Result

- New atoms: 3 exploratory (`BPT-071`, `LDG-027`, `LDG-028`).
- Existing/synonym coverage: all remaining applicable mechanisms.
- Unresolved disputes: 0.
- Canon/pending projection: regenerated from canonical rows; pending index enumerates exploratory rows separately from committed open work.
