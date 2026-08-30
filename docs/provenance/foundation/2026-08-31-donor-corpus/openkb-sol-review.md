# OpenKB supplemental SOL review

## Source boundary

- Repository: `VectifyAI/OpenKB`.
- Frozen clone: `C:/Users/adrds/AppData/Local/Temp/membrane-foundation-corpus-20260831/VectifyAI__OpenKB`.
- Commit: `ff54396e575ee6feb0113b631a34caa082b441cc`.
- License: Apache-2.0.
- Review: supplemental source-only SOL pass after original 26-repository dual-pass inventory; no cross-claim that original passes covered OpenKB.

## Disposition

| Mechanism | Source & live consumer | Canon result |
|---|---|---|
| Stable hash/path identity & dedupe | `openkb/state.py:11,121`; `openkb/converter.py:74,112`; `openkb/cli.py:451` | Existing `LDG-001`; donor alias history is weaker than `LDG-024`. |
| Multi-format normalization | `openkb/cli.py:229`; `openkb/converter.py:141`; live `_add_single_file_locked` | New exploratory `LDG-028`, narrowed to deterministic, grant-bound Markdown normalization with raw-source resolution, converter/version/config provenance, & typed loss/omission accounting. Behavioral reimplementation required. |
| Long-document hierarchy | `openkb/indexer.py:125,183`; `openkb/tree_renderer.py:38` | Existing `LDG-002`, `LDG-003`, `LDG-005`, `LDG-014`; stochastic PageIndex path at `openkb/indexer.py:206` closes nothing. |
| Semantic concept/entity compilation | `openkb/agent/compiler.py:975,1046,1121,1233,1263,1320,1403,1470,2205,2291` | Reference only for `LDG-023`, `CTX-033`, `PUL-034`; LLM-authored index truth is excluded. |
| Wikilink graph & hygiene | `openkb/visualize.py:22`; `openkb/lint.py:263,294,421`; `openkb/page_ops.py:41,62,106,168,194` | Existing `LDG-015`, `LDG-016`; visualization maps `MEM-028`. |
| Transactional rollback journal | `openkb/mutation.py:129,162,203,387,447`; `openkb/locks.py:102`; `openkb/add_coordinator.py:91` | Existing `LDG-017`, `LDG-018`; it does not prove ordered consumer catch-up. |
| Watch service | `openkb/watcher.py:17,50,105`; `openkb/watch_service.py:42,66,84,160` | Existing `BPT-019`, `BPT-021`, `BPT-043`; donor queue is volatile & non-replayable. |
| Raw-source view | `openkb/documents.py:59`; `openkb/api_documents_router.py:28` | Existing `LDG-005`, `LDG-006`; donor collapses some typed outcomes. |
| Model query traversal | `openkb/agent/query.py:56` | Excluded host-agent execution, not Blueprint/Ledger authority. |

## Exclusion fence

`LDG-028` does not absorb image extraction, PDF page rendering, stochastic structure synthesis, media ingestion, or multimedia embedding. Those remain outside Ledger under `docs/architecture/membrane.md:2045,2223`.

Result: zero Blueprint atoms, one exploratory Ledger atom (`LDG-028`), & durable catch-up merged into refined `LDG-023`.
