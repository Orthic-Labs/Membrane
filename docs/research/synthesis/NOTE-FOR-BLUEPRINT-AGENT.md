# Note to the Blueprint agent — membrane is a downstream consumer of your store

> **STATUS: ALL FOUR ASKS DELIVERED AND VERIFIED (2026-07-26).** Blueprint shipped `blueprint graph manifest` (envelope-only, read-only, never migrates) plus `openStoreReadOnly()`. Independently measured from membrane: **85 / 86 / 94 ms warm**, envelope carries every requested field including `sourceObservation{head,dirty,statusDigest}` and `storeSchemaVersion: 3`. WAL confirmed. Blueprint additionally found and fixed three defects membrane would have inherited: the manifest **undercounted its own generation by 47%** (33,487 recorded vs 62,743 stored edges — counts were computed pre-augmentation and never refreshed; membrane pins those numbers, so this would have poisoned freshness), a stale-immediately-after-build failure, and a triple-write of each generation. Membrane's pinned contract now lives in [`03-BUILD-PLAN.md`](03-BUILD-PLAN.md) §B0.3. The original asks are retained below as the record.

**From:** membrane/RightContext research pass, 2026-07-26. **Action needed from Blueprint's side: small; mostly "hold a stable surface and tell us when it moves."**

## What happened

Blueprint migrated to a single SQLite store `graph/graph.db` (nodes, edges, docTruth, and the manifest envelope) and removed `graph.json` with no fallback (`tools/skills/blueprint/SKILL.md:103`). Membrane's federation lane still reads the retired file contract in two places — `engine/federation/providers/blueprint.py:86-87` (`.agent/graph/manifest.json` / `.blueprint/manifest.json` → `generationId`) and `engine/crates/crypt/src/freshness.rs:538` (`.agent/graph/graph.json` → `graph_body_generation`). On new-Blueprint repos, membrane's central freshness verdict can't seal a Blueprint generation and the blueprint provider lane degrades on every prompt. Membrane owns that repair (task filed: both readers learn the SQLite store, legacy paths as fallback).

## What membrane needs from Blueprint going forward

1. **A stable, documented generation/envelope read surface.** Either (a) a documented, versioned schema for reading the manifest envelope + `generationId` directly from `graph.db` (table/column names membrane can pin), or (b) a cheap CLI read (`blueprint graph export --manifest-only` or equivalent) that emits just the envelope. Membrane needs `generationId`, base commit, and a body digest/generation for the graph content — read-only, sub-100 ms, safe to call from a prompt-path freshness check.
2. **Concurrent-read safety.** Membrane's freshness evaluator runs inside a 900 ms prompt hook; it will open `graph.db` read-only while `blueprint build` may be writing. Confirm WAL mode / read-only-connection semantics, and that a mid-build read yields either the previous complete generation or a typed "building" state — never a torn envelope.
3. **Change notice for store schema.** Membrane pins the exact sealed generation ("Blueprint candidates must return the exact generation sealed by the central verdict"). Any rename (e.g. a future `graph.bp`), envelope schema change, or generation-format change is a breaking downstream contract — a one-line entry in Blueprint's changelog naming the store path + schema version is enough for membrane's readers to fail loudly instead of silently degrading.
4. **Dirty-tree semantics.** Membrane distinguishes committed Blueprint snapshots from dirty-overlay state (a dirty build must not claim a committed snapshot). Preserve whatever field currently marks "built at commit X with clean/dirty status" in the new envelope.

## What Blueprint does NOT need to do

- No JSON file resurrection — membrane will read the DB.
- No API server, no push notifications — file/DB + changelog is fine.
- Nothing about membrane's internal lanes, budgets, or seals — those stay membrane-owned.
