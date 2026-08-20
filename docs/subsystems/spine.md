# Spine — Markdown Section Index

**Status:** canonical subsystem doctrine · draft for adoption
**Was:** "Markdown Doc Spine" / RMS D1–D4 (complete 2026-07-30, shadow-only)
**Code today:** `engine/crates/membrane-runtime/src/{doc_spine,doc_projection,doc_shadow,doc_candidate_provider}.rs`
**Parent:** `docs/subsystems/SYSTEM.md`

## Purpose
Answer one question: **where in the markdown is it?** An index over markdown files — like a database index — so an agent (or anything) goes to the exact section instead of reading the file.

## Owns
- `DocArtifactV1` registry: doc_id, repository root/id, revision, path, content hash, parser version, document class, lifecycle state, trust/influence/sensitivity labels, generated flag, index generation.
- Section-level projections with stable `anchor_id`s: `Lexical | WholeDocument | Section`, token counts, provenance (`collapsed_to_parent`).
- `sync(root)` with ignore rules, tombstones, health exclusion, invalidation by hash.
- `recall(query, k) → doc_id + source_ref + anchor_id + expected_hash + score`.
- Its own SQLite file (target). Regenerable from files.

## Does not own
Doc **truth** — whether a claim holds against code (Blueprint) · doc **memory** — what we learned from a doc (Cortex) · attention/admission (Planner) · repo-only scope: Spine indexes any markdown root the grant allows, including workspace docs Blueprint never sees.

## Public contract
- Doc-candidate provider to the Planner; each hit is a typed candidate carrying `source_ref + anchor_id + expected_hash`.
- Resolver-backed representation: the Planner may deliver the pointer and let `membrane_source_read` fetch the section on demand.
- Blueprint *may* later consume Spine for repo docs; Spine never consumes Blueprint.

## Invariants
1. A hit is a pointer + hash; delivering it never implies the section content was admitted.
2. Stale hash → `stale`, never silently served.
3. Projections for a doc are replaced transactionally per revision (no mixed-revision rows).
4. Lifecycle/trust labels are metadata for eligibility; Spine never assigns authority.
5. Own store file; rebuild wipes nothing outside Spine.

## Definition of Done
- [ ] Extracted to its own crate/module with its own store file; migration out of Cortex `MemDb`.
- [ ] Admitted (not shadow) under the Planner's evidence-class coverage floor; frozen fixtures prove non-regression.
- [ ] `recall` hits round-trip through `membrane_source_read` with hash verification.
- [ ] Sync is incremental, bounded, and reports a typed `DocSyncReport`.
- [ ] Workspace (non-repo) markdown roots supported under grant.
