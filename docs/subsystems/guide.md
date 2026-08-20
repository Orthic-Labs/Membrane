# Guide — Document Navigation Index

**Status:** derived subsystem reference · non-normative  
**Canonical name:** Guide  
**Historical name:** Spine / Markdown Doc Spine / RMS D1–D4  
**Parent system:** Membrane  
**Current implementation identifiers:** `doc_spine`, `doc_projection`, `doc_shadow`, `doc_candidate_provider` under Membrane runtime until a code-level rename is executed.

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

## Does not own

- document truth or code-vs-doc verification — Blueprint;
- durable learned knowledge from documents — Blueprint;
- final relevance/admission/attention policy — Membrane planner;
- source document authority or canonical content bytes;
- reduction/compression — Push.

## Public seam

Guide produces typed document candidates for the Membrane planner.

A Guide result is a pointer plus integrity metadata. The planner may admit the source content directly or use resolver-backed delivery to fetch the referenced section under the active grant.

Guide never upgrades source authority merely because a section was indexed.

## Invariants

1. A hit is a resolvable reference, not proof that its content was admitted.
2. Hash mismatch returns `stale`; stale content is never silently served as current.
3. Per-document projections update transactionally so readers do not observe mixed revisions.
4. Trust/influence/sensitivity metadata constrains eligibility; Guide does not assign authority.
5. Guide's index is rebuildable from source documents and does not share Blueprint's authored durable store.
6. Guide does not decide whether a document claim is true against code.
7. New documentation uses `Guide`; legacy `Spine` identifiers may remain only as implementation/history until deliberately renamed.

## Definition of Done

- [ ] `docs/subsystems/spine.md` is retired in favor of this file.
- [ ] Guide has a separately owned rebuildable index store/projection.
- [ ] Guide candidates can participate in the Membrane planner rather than remaining permanently shadow-only.
- [ ] A recalled section round-trips through source resolution with hash verification.
- [ ] Sync is incremental, bounded, and reports typed outcomes.
- [ ] Workspace/document roots outside repository code can be indexed only when the active grant permits them.
- [ ] Code-level `spine` identifiers are renamed only in a deliberate implementation change, not by documentation fiction.
