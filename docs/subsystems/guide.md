# Guide — Document Navigation Index

**Status:** derived subsystem reference · non-normative  
**Canonical name:** Guide  
**Historical name:** Spine / Markdown Doc Spine / RMS D1–D4  
**Parent system:** Membrane  
**Current implementation namespace:** `membrane-runtime::guide::{db, outline, identifier, doc_projection, doc_spine, doc_shadow, doc_candidate_provider}`.

## Purpose

Answer one question:

> **Where in the documents is the relevant material?**

Guide is a rebuildable navigation/index subsystem over document sources. The current implementation is Markdown-oriented. The name change to Guide does not by itself expand supported formats.

Guide does not own the canonical document bytes. It indexes them into stable, resolvable section references so the planner can retrieve the exact relevant region instead of reading entire files.

## Owns

- `DocArtifactV1`-style document registry metadata:
  document identity, source path/ref, revision/content hash, parser/index version, document class, lifecycle/index state, trust/influence/sensitivity labels, generated flag.
- Stable section anchors and section-level projections.
- Lexical/whole-document/section navigation projections.
- Incremental sync, invalidation, tombstones, and hash-bound staleness detection.
- `recall`-style document navigation returning:
  `doc_id + source_ref + anchor_id + expected_hash + score`.
- Its own rebuildable index store/projection.
- `GuideDb` at `cache_root()/guide-index.sqlite3`; this SQLite file is disposable and
  never opens Cortex's durable-memory database.

## Does not own

- document truth or code-vs-doc verification — Blueprint;
- durable learned knowledge from documents — Cortex;
- final relevance/admission/attention policy — Membrane planner;
- source document authority or canonical content bytes;
- reduction/compression — Push.

## Public seam

Guide produces typed document candidates for the Membrane planner.

A Guide result is a pointer plus integrity metadata. The planner may admit the source content directly or use resolver-backed delivery to fetch the referenced section under the active grant.

Guide never upgrades source authority merely because a section was indexed.

CLI surface: `membrane guide sync`, `membrane guide recall`, `membrane guide outline`,
and `membrane guide read`. There is no top-level document command or generic recall
fallback into Guide.

## Invariants

1. A hit is a resolvable reference, not proof that its content was admitted.
2. Hash mismatch returns `stale`; stale content is never silently served as current.
3. Artifact rows and per-document projections update in one SQLite transaction so readers do not observe mixed revisions.
4. Trust/influence/sensitivity metadata constrains eligibility; Guide does not assign authority.
5. Guide's index is rebuildable from source documents and does not share Cortex's authored durable store.
6. Guide does not decide whether a document claim is true against code.
7. New documentation & code use `Guide`; `Spine` is historical terminology only.

## Definition of Done

- [x] `docs/subsystems/spine.md` is retired in favor of this file.
- [x] Guide has a separately owned rebuildable index store/projection.
- [ ] Guide candidates can participate in the Membrane planner rather than remaining permanently shadow-only.
- [ ] A recalled section round-trips through source resolution with hash verification.
- [ ] Sync is incremental, bounded, and reports typed outcomes.
- [ ] Workspace/document roots outside repository code can be indexed only when the active grant permits them.
- [x] Guide implementation & tests live under a discoverable `guide` namespace without compatibility wrappers.
