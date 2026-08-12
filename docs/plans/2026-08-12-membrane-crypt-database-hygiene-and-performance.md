# Membrane and Crypt runtime, database hygiene, and performance

Date: 2026-08-12
Status: inventory-corrected implementation plan; no live database mutation performed
Systems: Membrane/Crypt primary; workspace installation secondary; Cortex excluded

## Objective

Reduce measured Membrane/Crypt latency and growth while giving every owned SQLite database one canonical owner, path, lifecycle, WAL policy, recovery procedure, and resource budget. Durable Crypt memory remains non-disposable.

## Boundary from Cortex

Cortex's graph is derived and may be rebuilt into a fresh sidecar. `crypt-engine.db` contains durable user memory and event history; it must be backed up, logically validated, and recoverably adopted. Cortex schema compaction remains in the separate Cortex storage book.

## Current measured state

| Host | Database | Main bytes | WAL bytes | Integrity | Important state |
|---|---|---:|---:|---|---|
| Mac | workspace-root `catalog.db` | 4,096 | 74,192 | OK | Seven tables, all zero rows; stale WAL/SHM |
| Mac | `tools/.cache/memory/catalog.db` | 73,728 | 37,112 | OK | One grant, receipt, and retrieval event |
| Mac | `tools/.cache/memory/crypt-engine.db` | 253,059,072 | 4,132,392 | OK | 2,516 memories; 27,046 free pages |
| Mac | `tools/.cache/memory/context-telemetry-outbox.db` | 11,718,656 | 0 | OK | 8,192 queued rows; checkpointed |
| Windows | workspace-root `catalog.db` | 4,096 | 74,192 | OK | Same empty/stale shape as Mac |
| Windows | `tools/.cache/memory/catalog.db` | 73,728 | 0 | OK | Checkpointed |
| Windows | `tools/.cache/memory/crypt-engine.db` | 49,930,240 | 7,024,632 | OK | 2,420 memories; one free page |
| Windows | `tools/.cache/memory/context-telemetry-outbox.db` | 12,496,896 | 0 | OK | Checkpointed |

All measured stores use 4,096-byte pages and report WAL mode.

Mac `crypt-engine.db` has 27,046 free pages, about 110.8 MB or 43.8% of the main file. This is genuine reclaimable internal space, unlike the Cortex graph. A safe compaction could approach 142 MB, but only a measured copy-and-adopt run may establish the result.

Mac is also carrying more durable and projected content: 9,620 `doc_projections` and 18,522 `doc_artifacts`, versus 2,661 and 2,794 on Windows. Host size difference is therefore both real content and Mac free-page accumulation.

`doc_projections` occupies about 108.0 MB on Mac and 28.8 MB on Windows. Its derivation, retention, and rebuild contract needs proof before any pruning.

The top-level Mac cache contains `test-old-runtime.db-wal` and `test-old-runtime.db-shm` without a matching main DB. They are orphan candidates, not automatic deletion targets.

## Existing inventory that must be reused

- `doc_artifacts` already stores path, `content_hash`, and `parser_version` in `engine/crates/membrane-runtime/src/doc_spine.rs`. Document sync does not query that table before parsing unchanged files. Add one lookup path; do not create another manifest.
- `run_capped` in `engine/crates/membrane-runtime/src/runc.rs` already preserves bounded head/tail output and exit status, but captures full output in memory and always writes a spill file. Work remaining is streaming capture plus spill creation only after cap breach.
- Federation provider code is subprocess-heavy: `live.py` invokes seven subprocesses and Crypt/anchors providers invoke two each. Waiting on subprocesses releases Python's GIL, so CPU starvation is not established by inspection.
- FTS5-backed lexical retrieval, a bounded LRU for repeated provider results, worker-thread execution, and prefix enforcement are genuinely absent from the inspected implementation.
- Repository rules require warm federation measurement before gateway concurrency or budget changes. That measurement gates concurrency work.

## Delivery law — equivalence before optimization

No performance implementation begins until canonical CI freezes packet contents/order, typed omissions/degradation, grants, freshness/generation identity, cancellation, timeout behavior, no-op document sync, command exit/output behavior, and warm federation benchmark receipts.

Every later change lands independently and must prove identical observable behavior with less work. A failed gate rejects that change; it never weakens expected packets, typed errors, deadlines, fixtures, or thresholds.

Resident-worker isolation and gateway concurrency remain measurement-gated. Initial command adapters are limited to Git and repository test runners; broader adapters require a demonstrated recurring parsing need and a separate contract.

## Canonical storage contracts found in source

- Crypt durable store: `<workspace>/tools/.cache/memory/crypt-engine.db`, overridable by `CRYPT_DB`.
- Rust context catalog: `<context-home>/catalog.db`; `CONTEXT_HOME` wins, otherwise resolution derives from `CRYPT_DB` or environment home.
- Python scope-grant reader: `RIGHTCONTEXT_CATALOG`, defaulting to `~/.claude/rightcontext/catalog.db`.
- Telemetry outbox: sibling `context-telemetry-outbox.db` under the memory cache unless explicitly overridden.
- Installed service state identifies the workspace Crypt DB and native lifecycle owner.

Rust and Python catalog defaults are not one canonical resolver. The empty workspace-root `catalog.db` appearing identically on both hosts is evidence of historical or alternate configuration, but not proof that deletion is safe.

## Problems to resolve

1. Two catalog files with the same schema can exist under different paths, while Rust and Python defaults disagree.
2. Both hosts have an empty 4 KB workspace-root catalog whose 74 KB stale WAL holds uncheckpointed schema state.
3. Orphan sidecars exist without a matching main file on Mac.
4. No single inventory receipt binds active process, canonical main file, WAL, SHM, schema version, and installation identity.
5. Mac Crypt contains about 110.8 MB of reclaimable free pages.
6. Crypt and catalog open WAL with sensible PRAGMAs but do not verify the effective mode after requesting it.
7. WAL checkpoint ownership and observable starvation thresholds are not explicit across every store.
8. Catalog access is mutex-serialized; Crypt read concurrency and contention need measurement before adding handles or pools.
9. Projection and outbox retention must remain bounded without deleting durable facts.

## Existing good shape

- Crypt and catalog already request WAL, `synchronous=NORMAL`, 5-second busy timeout, and in-memory temporary storage.
- Catalog is failure-isolated from the Crypt durable store.
- Context telemetry outbox is currently checkpointed on both hosts.
- Crypt health is native-service-owned: launchd on Mac and Task Scheduler on Windows.
- Membrane hooks perform health checks only and do not own service lifecycle.

## Pending improvements, prioritized

### P0 — measure warm federation before changing concurrency

- Freeze a representative warm request corpus, provider mix, host/toolchain identity, timeout budget, source commit, and raw timing/RSS receipts.
- Attribute wall time to gateway scheduling, each subprocess launch/wait, provider CPU, serialization, and result merge.
- Measure sequential and bounded-concurrency variants without changing production defaults.
- Do not attribute waits to GIL starvation without profiler evidence.
- Advance gateway concurrency only when p95 latency improves without timeout, cancellation, RSS, determinism, or typed-degradation regression.

### P1 — skip unchanged document artifacts through existing state

- Query `doc_artifacts` by canonical path before parsing.
- Skip only when both content hash and parser version match current values.
- Preserve deletion detection, parser-version invalidation, transactionality, and projection ownership.
- Add changed, unchanged, deleted, parser-upgrade, interrupted-sync, and duplicate-path tests.
- Emit counters for scanned, hashed, parsed, skipped, deleted, and invalidated artifacts.

### P2 — finish streaming command capture

- Keep current head/tail cap and exit-code preservation.
- Stream stdout and stderr into bounded head/tail buffers while counting bytes.
- Create and write a spill file only after output exceeds the in-memory cap.
- Preserve cancellation, timeout, UTF-8 handling, platform shell resolution, and exact exit status.
- Add explicit adapters only for Git and repository test runners in this plan; keep generic execution bounded.

### P3 — add lexical and repeated-read indexes where measured

- Add FTS5 only for a frozen lexical retrieval contract with deterministic fallback when FTS5 is unavailable.
- Add a size-bounded LRU only where repeated provider inputs have stable cache and invalidation keys.
- Record cache key, invalidation key, reused work, capacity, eviction, and hit/miss counters.
- Never cache authority, freshness, grant, or generation decisions across their identity changes.

### P4 — enforce prefixes at the owning boundary

- Define canonical accepted prefixes in one parser/validator.
- Reject ambiguous, malformed, or cross-repository identifiers with typed errors.
- Apply validation before filesystem, database, or subprocess work.
- Add traversal, alias collision, Unicode, case, and Windows-path fixtures.

### P5 — add workers only after P0 evidence

- Keep cancellation, deadlines, deterministic merge order, and bounded fan-out invariant.
- Use worker threads only for measured CPU-bound Python work; subprocess waits remain async/bounded scheduling work.
- Cap workers from measured RSS and active-provider count, not logical CPU count alone.
- Retain sequential fallback on worker failure or small workloads.

### P6 — canonicalize database identity and paths

- Implement one catalog-path resolver consumed by Rust service, Python grant reader, installers, health output, and operational tools.
- Persist resolved Crypt, catalog, outbox, workspace, and installation identities in the runtime receipt.
- Require absolute paths and reject accidental current-directory fallback for production.
- Add a catalog installation identifier or equivalent metadata so two schema-identical files cannot be mistaken for one store.
- On startup, report canonical path, effective journal mode, schema version, main/WAL sizes, and whether another same-schema catalog exists.
- Never merge or delete duplicate catalogs automatically; classify authority from runtime binding and row provenance first.

### P7 — build a safe hygiene command

- Inventory main, WAL, SHM, owner process, open file handles, schema version, integrity, row counts, timestamps, and configured path without creating missing databases.
- Open probes with read-only URI mode; never let an inspection command create a zero-byte DB.
- Use SQLite backup APIs for a live consistent backup; never copy only the main file while WAL is active.
- Classify sidecars as active, recoverable, stale, orphan candidate, or quarantined.
- Quarantine confirmed orphan files to a dated recoverable directory before deletion.
- Resolve the two empty workspace-root catalogs only after proving no installed process or configuration owns them.

### P8 — compact durable Crypt safely

- Stop the native Crypt service at a declared maintenance boundary.
- Create a consistent backup with main, schema, user version, row-count, key-set, and integrity receipts.
- Use `VACUUM INTO` or an equivalent new-file compaction; never vacuum the only durable copy in place.
- Validate `integrity_check`, foreign keys, schema/user version, critical-table counts, memory identity keys, event-log continuity, and recall smoke tests against the compacted file.
- Atomically adopt the validated compacted file with a rollback copy, then restart the native service and verify health.
- Run independently on Mac and Windows; never copy one host's durable memory database onto the other.

### P9 — define WAL and checkpoint ownership

- Verify effective `journal_mode` after requesting WAL and emit typed degradation if the filesystem refuses it.
- Let writer/maintenance ownership perform checkpoints; read-only hooks and consumers never checkpoint or migrate.
- Use passive checkpoints after bounded committed batches or idle thresholds.
- Use truncate checkpoints only at clean shutdown or a proven quiescent maintenance boundary.
- Record WAL bytes, uncheckpointed frames, checkpointed frames, busy result, oldest active reader, and checkpoint duration.
- Keep 5-second timeouts for latency-budget reads; benchmark a 30-second background-writer timeout separately.
- Treat `journal_size_limit` as post-checkpoint retention, not a hard growth cap.

### P10 — measure read-path tuning

- Benchmark read-only `query_only` handles with 256 MB mmap and a bounded page cache against current recall latency.
- Measure aggregate RSS before raising per-connection cache sizes.
- Keep one writer; add read handles only if recall demonstrably queues behind writes.
- Do not add a generic connection pool without measured concurrent-reader demand.
- Keep schema migration writer-owned; readers fail with a typed version mismatch.

### P11 — prove projection and outbox retention

- Classify `doc_projections` as durable, reproducible, or mixed before pruning or rebuilding it.
- Bind every projection to source artifact, version, and reconstruction rule.
- Measure why Mac carries 9,620 projections versus Windows 2,661.
- Keep telemetry outbox capacity, retry state, and acknowledged-row pruning explicit; its current zero-byte WAL is healthy evidence, not a reason to redesign it.

## Ownership

### Membrane/Crypt

- Own durable schema, catalog identity, WAL policy, logical backup, compaction validation, recall parity, and service health.

### Workspace installation

- Own canonical installed paths, runtime receipts, native service stop/start, hygiene command, and Mac/Windows orchestration.

### RightKit and Orthic

- RightKit owns native build, signing, sealing, and publication when Membrane engine code changes.
- Orthic adopts a new signed Membrane add-on only when a binary change requires a release.
- Data-only maintenance does not trigger an app or installer rebuild.

## Acceptance gates

Path and ownership:

- Rust, Python, installer, health, and operational tools resolve the same canonical catalog on each host.
- No production path falls back to the current working directory.
- Every duplicate or orphan candidate has an owner/provenance disposition and recoverable quarantine receipt.

Durability:

- Pre/post critical-table counts and key-set digests match after compaction.
- Memory identities, event-log ordering, deletion records, feedback, skills, and recall behavior remain intact.
- Failure at every pre-adoption step leaves the original DB active and readable.
- Post-adoption rollback restores the exact prior logical state.

Performance and resources:

- Warm federation report distinguishes subprocess wait, provider CPU, scheduling, serialization, and merge time.
- Any concurrency change beats frozen sequential p95 without increasing timeout/error rate, RSS ceiling, or result variance.
- Unchanged document sync performs hashing plus one indexed artifact lookup and does not parse or rewrite matching artifacts.
- `run_capped` retains bounded memory, spills only after cap breach, and preserves full output plus exit status in spill mode.
- FTS5, LRU, and workers land only with focused before/after measurements and explicit fallback behavior.
- Mac Crypt main file reflects reclaimed free pages without forced target-size claims.
- Recall latency does not regress; report p50, p95, cold, and concurrent-write measurements.
- RSS is measured for service, read handles, mmap, and cache changes.
- WAL returns to zero after a quiescent checkpoint and remains observable when a reader blocks progress.

Cross-platform:

- Mac launchd and Windows Task Scheduler health pass after maintenance.
- Native tests pass on both hosts.
- No database is transferred between hosts.
- Any Rust binary change receives a new native Mac and Windows signed Membrane patch release before Orthic adoption.

## Execution order

1. Land packet, typed-degradation, no-op sync, command behavior, cancellation, and warm benchmark equivalence gates in CI.
2. Freeze live database path, owner, schema, row-count, allocation, WAL, and process receipts on both hosts.
3. Land existing-`doc_artifacts` unchanged-file skipping and focused sync tests.
4. Finish streaming `run_capped`; spill only after cap breach; limit adapters to Git and test runners.
5. Select measured FTS5/LRU/prefix work; add workers or gateway concurrency only if P0 evidence justifies them.
6. Land canonical path resolution and read-only inventory without touching live data.
7. Classify duplicate catalogs and orphan sidecars; quarantine only after liveness proof.
8. Add effective-WAL verification, checkpoint metrics, and writer-owned maintenance policy.
9. Prove projection and outbox retention contracts.
10. Rehearse backup, compact, validate, adopt, rollback, and restart on fixtures.
11. Compact Mac Crypt once; measure size, recall, RSS, WAL, and health.
12. Compact Windows Crypt only if measured free pages justify it; current evidence says they do not.
13. If source changed, build and sign Membrane natively on both hosts, publish through GitHub Releases, then adopt through Orthic.

This order freezes behavior before optimization, measures before changing federation, reuses existing artifact and head/tail machinery, and keeps durable-memory maintenance separate from Cortex's disposable graph rebuild.
