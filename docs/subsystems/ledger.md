# Ledger — Document Navigation Index

**Status:** derived subsystem reference · non-normative  
**Canonical name:** Ledger  
**Historical name:** Spine / Markdown Doc Spine / RMS D1–D4  
**Parent system:** Membrane  
**Current implementation namespace:** `membrane-runtime::ledger::{db, outline, identifier, doc_projection, doc_spine, doc_shadow, doc_candidate_provider}`.

## Purpose

Answer one question:

> **Where in the documents is the relevant material?**

Ledger is a rebuildable navigation/index subsystem over document sources. The current implementation is Markdown-oriented. The name change to Ledger does not by itself expand supported formats.

Ledger does not own the canonical document bytes. It indexes them into stable, resolvable section references so the planner can retrieve the exact relevant region instead of reading entire files.

## Owns

- `DocArtifactV1`-style document registry metadata:
  document identity, source path/ref, revision/content hash, parser/index version, document class, lifecycle/index state, trust/influence/sensitivity labels, generated flag.
- Stable section anchors and section-level projections.
- Lexical/whole-document/section navigation projections.
- Incremental sync, invalidation, tombstones, and hash-bound staleness detection.
- `recall`-style document navigation returning:
  `doc_id + source_ref + anchor_id + expected_hash + score`.
- Its own rebuildable index store/projection.
- `LedgerDb` at `cache_root()/ledger-index.sqlite3`; this SQLite file is disposable and
  never opens Cortex's durable-memory database.

## Does not own

- document truth or code-vs-doc verification — Blueprint;
- durable learned knowledge from documents — Cortex;
- final relevance/admission/attention policy — Membrane planner;
- source document authority or canonical content bytes;
- reduction/compression — Push.

## Public seam

Ledger produces typed document candidates for the Membrane planner.

A Ledger result is a pointer plus integrity metadata. The planner may admit the source content directly or use resolver-backed delivery to fetch the referenced section under the active grant.

Ledger never upgrades source authority merely because a section was indexed.

CLI surface: `membrane ledger sync`, `membrane ledger recall`, `membrane ledger outline`,
and `membrane ledger read`. There is no top-level document command or generic recall
fallback into Ledger.

## Invariants

1. A hit is a resolvable reference, not proof that its content was admitted.
2. Hash mismatch returns `stale`; stale content is never silently served as current.
3. Artifact rows and per-document projections update in one SQLite transaction so readers do not observe mixed revisions.
4. Trust/influence/sensitivity metadata constrains eligibility; Ledger does not assign authority.
5. Ledger's index is rebuildable from source documents and does not share Cortex's authored durable store.
6. Ledger does not decide whether a document claim is true against code.
7. New documentation & code use `Ledger`; `Spine` is historical terminology only.

## Definition of Done

- [x] `docs/subsystems/spine.md` is retired in favor of this file.
- [x] Ledger has a separately owned rebuildable index store/projection.
- [ ] Ledger candidates can participate in the Membrane planner rather than remaining permanently shadow-only.
- [ ] A recalled section round-trips through source resolution with hash verification.
- [ ] Sync is incremental, bounded, and reports typed outcomes.
- [ ] Workspace/document roots outside repository code can be indexed only when the active grant permits them.
- [x] Ledger implementation & tests live under a discoverable `ledger`
      namespace without current-product compatibility wrappers.
- [ ] The rename cutover has fresh scoped Rust build/test evidence; the prior
      pre-Hub/protocol compile is not current-head proof.


## Naming

Ledger was named Guide (and Spine before that). Guide is retired as a current-product name. The old `guide-index.sqlite3` file and `guide_doc_*` tables are recognized only at the explicit one-release upgrade boundary, where the disposable index is retired and rebuilt under Ledger naming. Canonical detail lives in `LEDGER-MARKDOWN-INDEXING-AND-DOCUMENT-NAVIGATION-CANON.md`.
