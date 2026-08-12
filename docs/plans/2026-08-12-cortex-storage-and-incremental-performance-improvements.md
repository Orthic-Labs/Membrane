# Cortex build, storage, and incremental-performance improvements

Date: 2026-08-12
Status: inventory-corrected implementation plan; correctness repairs are landed
Systems: Cortex primary; Membrane search seam secondary; no Orthic or release-artifact change

## Objective

Reduce Cortex cold-build time, graph storage, incremental-update latency, WAL growth, and resident-resource cost without weakening global resolution soundness, deterministic search, generation binding, atomic adoption, or Membrane's typed degradation behavior.

## Current measured state

| Host | DB bytes | Display size | WAL | Files | Symbols | `symbol_terms` rows |
|---|---:|---:|---:|---:|---:|---:|
| Mac | 1,854,156,800 | 1.85 GB / 1.73 GiB | 0 | 3,975 | 163,368 | 2,586,494 |
| Windows | 3,565,207,552 | 3.57 GB / 3.32 GiB | 0 | 11,050 | 292,930 | 4,788,038 |

`symbol_terms` plus `idx_symbol_terms_symbol` consumes 927,891,456 bytes on Mac and 1,810,825,216 bytes on Windows: 50.0% and 50.8% of each database.

The secondary index alone consumes 462,319,616 bytes on Mac and 902,238,208 bytes on Windows. Average external symbol IDs are 92.7 bytes on Mac and 105.8 bytes on Windows; `symbol_terms.symbol_id` averages 99.0 and 110.4 bytes, while tokens average only 6.5 and 8.0 bytes.

Free pages are only about 9.2 MB on Mac and 0.6 MB on Windows. `VACUUM` alone cannot materially reduce either database.

The last Mac four-path incremental catch-up took about five minutes. This is live evidence that incremental search maintenance remains too expensive after contamination and WAL repairs.

For Cortex core excluding competitor snapshots, a local 550-file run measured 23.4 seconds and 899 MB RSS. Lexical resolution consumed 21.2 seconds while Tree-sitter consumed 2.2 seconds. A one-file delta measured 77.9 ms and a no-op barrier 2.7 ms. These measurements remain baseline observations until their exact commands, fixture identity, host identity, and raw reports are committed beside the benchmark harness.

Windows is larger primarily because its graph covers 11,050 files versus 3,975 on Mac. Both graphs contain zero `node_modules` or benchmark-directory rows.

Mac currently contains 177 symbols outside its primary generation; Windows contains 39. This residue is negligible today but needs a bounded invariant so incremental generations cannot accumulate silently.

## Existing inventory that must be reused

- `graph/delta-store.mjs` already owns the Merkle ledger, file state, artifact state, `MAX_HOPS = 2`, and `MAX_DEPENDENT_FILES = 500`. Do not build a second incremental manifest or duplicate these caps.
- `graph/parse-cache.mjs` deliberately caches harvested facts while globally re-resolving them against the current symbol universe. Global re-resolution is a soundness invariant that prevents ghost edges when symbols move, appear, or disappear.
- Existing parse reuse does not make global resolution cheap: repeated file and symbol scans remain the dominant cold-build cost.
- `service/protocol.mjs` currently exposes query methods only. `service/server.mjs` already owns cancellation and a per-root queue for those reads, but no build method or cross-caller build singleflight exists.
- Existing daemon cancellation and resource-limit behavior must be extended, not reimplemented through a separate lock-file protocol.
- Existing exact in-process query latency is already below 5 ms. CLI/process startup, not query execution, dominates external latency.

## Delivery law — equivalence before optimization

No performance implementation begins until canonical CI contains these regression gates:

- frozen retrieval corpus with ordered outputs, hit rate, MRR, omissions, and generation identity;
- focused suite baseline reproduced from canonical `main` with raw command receipts;
- byte-identical no-op rebuild output and generation identity;
- explicit ghost-edge fixtures for symbol add, delete, move, rename, and ambiguity changes;
- cancellation, interrupted publication, and poisoned-snapshot recovery fixtures;
- canonical cold, no-op, and one-file-delta benchmark manifests with host/toolchain identity and raw samples.

Every later change lands independently and must prove identical observable outputs with less work. A failed equivalence gate rejects that change; it never causes baseline weakening, fixture deletion, ordering relaxation, or a larger tolerance. Remove the 4× CI slack only after stable host-normalized baselines exist.

Fact-level bounded resolved-edge caching is not part of this plan. Indexed global resolution is final shape unless measurements after P0 still miss gates; any replacement requires a new soundness decision and its own equivalence proof.

## What was wrong

1. Parcel subscription exclusions resolved to existing literal paths. Excluded directories created after subscription could enter the graph.
2. Duplicate watcher events reached SQLite before coalescing.
3. Watcher callbacks repeatedly opened the same database and could overlap gap reconciliation.
4. Applied journal rows and WAL frames accumulated when checkpoints could not progress.
5. Rebuilds modified the live store instead of constructing and validating a separate inode.
6. Full rebuild search population performed per-symbol replacement work after the store had already been cleared.
7. Searchable symbol information exists in both FTS `symbol_search` and portable `symbol_terms`.
8. `symbol_terms` is `WITHOUT ROWID` but its composite primary key repeats TEXT `generation_id`, token, and TEXT `symbol_id` millions of times; its secondary symbol index repeats more of that data.
9. TEXT IDs and generation IDs repeat across `symbols`, `edges`, `fact_owner`, search tables, and their indexes.
10. Incremental updates still delete and reinsert search state per affected symbol, multiplying writes by token count.

## Repairs already landed

- `a04307e`: coalesce events before journal persistence, reuse one actor DB handle, serialize gap repair, checkpoint passively after committed batches, prune applied journal history, and tune readers with 256 MB mmap plus 64 MB cache.
- `7b87355`: filter ignored paths at actor ingress.
- `e03a14e`: load configured exclusions dynamically, remove full-rebuild per-symbol search deletion, build into a fresh database, validate integrity and identity, truncate its WAL, and atomically adopt it.
- Mac and Windows graphs were rebuilt from current source.
- Current result on both hosts: fresh graph, zero pending paths, zero event gap, zero contamination, and zero-byte WAL after quiescent checkpoint.

## Pending improvements, prioritized

### P0 — index global resolution without weakening it

- Build and reuse `filesByPath`, `symbolsByName`, `importsByFile`, and equivalent schema/config indexes once per build.
- Iterate harvested call names and their candidate source symbols instead of every source symbol against every distinct target name.
- Hoist shared indexes across import, call, schema, and config edge passes.
- Preserve global re-resolution against the complete current symbol universe. Do not cache resolved edges behind a two-hop/500-file closure.
- Keep raw harvested facts content-addressed in the existing parse/delta stores; invalidate by content hash and parser/extractor version.
- If a future bounded resolution path is proposed, require a typed truncation result plus deterministic full-resolution fallback before adoption.
- Remove the duplicate consecutive `computeGenerationId(cleanNodes, cleanEdges, ...)` serialization/hash.

### P1 — add daemon-owned build singleflight

- Extend the existing daemon protocol with a build operation using the same typed request/response and cancellation conventions.
- Join concurrent equivalent builds by canonical `(repo, outDir, source fingerprint)` identity.
- Put build ownership behind the daemon's existing canonical-root coordination; do not add an independent lock-file authority.
- Let a newer incompatible source fingerprint cancel and replace stale in-flight work only through the existing cancellation path.
- Prove waiter cancellation does not kill work still needed by another waiter and process termination cannot publish a partial generation.
- Keep exact-symbol routing on the resident path so process startup does not dominate query latency.

### P2 — one scan, one hash, one publication

- Reuse file bytes across document, lexical, and AST providers where ownership permits.
- Hash staged generation content once after augmentation.
- Publish compact sidecars from the staged generation; do not pretty-print duplicate multi-megabyte graph bodies.
- Preserve atomic adoption, generation identity, and poisoned-snapshot recovery.

### P3 — remove unused FTS and normalize keys

A repository consumer sweep finds no `MATCH` query against `symbol_search`; `symbolSearchIsFts` has no caller, and `searchGenerationSymbols` reads only `symbol_terms`. Remove it only after public-schema and fixed-corpus compatibility tests.

- Stop inserting and deleting `symbol_search` rows.
- Remove its migration/table only after confirming no supported external DB consumer exists.
- Measure isolated size and incremental-write effect before combining results with integer compaction.
- Keep `symbol_terms` as canonical path because it already owns deterministic longest-token lookup, valid-generation filtering, and `*` fallback.
- Reconsider compact contentless FTS only if a future supported consumer requires ranking or substring search.
- Introduce `generation(id INTEGER PRIMARY KEY, external_id TEXT UNIQUE, ...)`.

- Introduce `generation(id INTEGER PRIMARY KEY, external_id TEXT UNIQUE, ...)`.
- Give symbols an integer row key; retain external string identity once only when API compatibility requires it.
- Replace repeated `generation_id TEXT` with `generation_row INTEGER` foreign keys.
- Replace `symbol_terms.symbol_id TEXT` with `symbol_row INTEGER`.
- Introduce a token dictionary and replace repeated `token TEXT` with `token_row INTEGER` when its measured join cost passes the query corpus.
- Convert edge, provider-owner, and search indexes to integer references where query contracts allow it.
- Keep migration backups and N-2/N-1 migration fixtures.

Dropping FTS alone will not halve the DB because `symbol_terms` and its secondary index dominate allocation. Near-50% reduction requires compacting that representation too. Any percentage estimate remains a planning hypothesis, not acceptance evidence.

### P4 — prepare and batch SQLite writes

- Remove `idx_symbol_terms_symbol` after its delete contract has a replacement; SQLite does not duplicate the indexed symbol column twice, but the measured index still repeats the remaining composite primary-key columns and occupies about one quarter of each database.
- Retain each symbol's prior token-row list in a compact ledger or blob, then delete removed terms through exact `(generation, token, symbol)` primary-key probes.
- Collect all affected symbol rows before opening the write transaction.
- Use one `BEGIN IMMEDIATE` transaction per coalesced file batch.
- Delete search rows by affected integer symbol rows or affected paths, not by the entire generation.
- Insert replacement terms with reused prepared statements and bounded batches.
- Use a staging table when it beats direct affected-row replacement.
- For a full rebuild only, clear once, bulk insert, then create or rebuild secondary indexes.
- Never run `DELETE WHERE generation_id=?` for an ordinary incremental file update.
- Prepare statements once per writer, register provider ranks once, stage compact rows, and keep one serialized SQLite writer.

### P5 — bound WAL and journal behavior

- Retain WAL mode, passive post-batch checkpointing, applied-journal pruning, and truncate only at atomic adoption or an explicit quiescent maintenance boundary.
- Add metrics for WAL bytes, oldest active reader, checkpointed frames, busy checkpoints, batch duration, and unapplied journal age.
- Add a threshold-triggered maintenance signal; report checkpoint starvation instead of claiming a hard WAL cap.
- Benchmark `wal_autocheckpoint` values including SQLite default and 4,000 pages.
- Do not claim `wal_autocheckpoint=4000` prevents a multi-GB WAL; long-lived readers can still block progress.
- Consider `synchronous=NORMAL` only after explicitly accepting possible loss of recent committed transactions during power failure. Database consistency is not identical to transaction durability.

### P6 — benchmark writer and page-layout tuning

- Separate connection policies by role: disposable rebuild sidecar, live watcher writer, and read-only consumer.
- For the disposable sidecar only, benchmark `journal_mode=OFF`, `synchronous=OFF`, exclusive locking, and a larger cache; any failure discards the sidecar, and WAL must be restored and read back before integrity validation and adoption.
- Benchmark writer `cache_size=-131072`, `mmap_size=268435456`, and current defaults.
- Measure aggregate RSS across every resident repo actor; a 128 MB cache must not multiply unboundedly per actor.
- Benchmark `temp_store=MEMORY`; reject it if memory pressure or concurrent rebuild RSS regresses.
- Run `ANALYZE` and `PRAGMA optimize` once after rebuild or substantial schema change, not after each batch.
- Buffer terms in a staging table, sort by final primary key, bulk-load sequentially, and create secondary indexes after the load.
- Verify the effective `journal_mode` after requesting WAL; external or network filesystems may refuse it without a hard error.
- Set `query_only=ON` on read-only consumers and keep schema migration writer-owned.
- Benchmark page sizes 4,096, 8,192, and 16,384 bytes on both machines.
- Apply a page-size change only during the single planned schema rebuild; larger pages can improve B-tree depth but increase small-write amplification.

### P7 — parallelize outside SQLite's writer boundary

- Parallelize parsing, provider extraction, tokenization, and descriptor construction with bounded workers.
- Keep one serialized SQLite writer and one `BEGIN IMMEDIATE` batch at a time.
- Transfer compact prepared rows to the writer instead of raw file bodies where possible.
- Size worker concurrency from measured CPU and memory pressure, not logical-core count alone.
- Add bounded workers only after resolver, publication, and writer changes are measured; parsing currently consumes under 10% of measured cold-build time.

## Cross-system seam requirements

### Cortex

- Own schema, migrations, search implementation, batch writer, WAL policy, metrics, rebuild, and atomic adoption.

### Membrane

- Preserve the existing typed search response, valid-generation requirement, deterministic fallback, deadlines, and honest unavailable/degraded states.
- Change only the Cortex query adapter needed for the selected canonical search representation.
- Add parity tests against old and new stores before deleting either search representation.

### Workspace and operating systems

- Implement and test source changes on Mac first.
- Rebuild and measure the Mac graph only after schema and query behavior are final.
- Pull the same source on Windows, run native tests, then rebuild and measure Windows once.
- Do not rebuild Membrane or Orthic signed release artifacts; this is local Cortex graph storage and query work.

## Acceptance gates

Correctness:

- Fixed query corpus returns identical ordered symbol identities, generation filtering, fallback behavior, and typed omissions on old and new implementations.
- Migration rollback, interrupted migration, integrity check, manifest identity, and atomic-adoption tests pass.
- Cortex full suite, Membrane seam tests, and Mac/Windows native graph checks pass.

Storage:

- No `node_modules` or benchmark-directory graph rows.
- No material free-page explanation is used as a substitute for schema reduction.
- Old-generation symbol and term rows remain within an explicit bounded residue gate.
- Planning targets: Mac 0.85–1.10 GB; Windows 1.60–2.00 GB. Report actual values rather than forcing these estimates.

Performance:

- Commit a reproducible baseline manifest containing command, source commit, fixture identity, host/toolchain identity, samples, raw timings, and peak RSS.
- A 550-file cold build targets less than 5 seconds and less than 300 MB RSS; report misses honestly rather than weakening the gate.
- No-change rebuild is less than 1 second; one-file delta p95 is less than 100 ms.
- Exact symbol lookup p95 is less than 5 ms through the resident daemon.
- A committed 5,000-file fixture cold-build gate is less than 60 seconds and less than 1 GB RSS.
- Four-path incremental update completes in seconds, not minutes, on both hosts.
- A fixed 100-file update corpus improves by at least 10× from the recorded baseline without changing results.
- Idle watcher CPU returns near baseline after drain; no stuck writer or duplicate reconcile remains.
- Aggregate watcher RSS is measured before and after every cache or worker change.
- Remove the 4× CI slack only after stable host-normalized baselines exist.

WAL and recovery:

- WAL returns to zero after explicit quiescent checkpoint on both hosts.
- A blocked-reader fixture proves growth is observable and typed; no configuration is described as a guaranteed hard cap.
- Process interruption during build leaves the previous validated DB readable.

## Execution order

1. Land equivalence, ghost-edge, no-op byte identity, cancellation, and canonical benchmark gates in CI.
2. Replace quadratic global resolver scans with shared indexes; retain global soundness and prove edge parity.
3. Remove duplicate generation serialization/hash and publish one staged generation.
4. Measure gates. Stop if targets pass; do not add fact-level closure caching.
5. Extend resident daemon with cancellable build singleflight and exact-symbol fast routing.
6. Prove FTS has no supported consumer; prototype integer-keyed terms, token dictionary, and secondary-index-free exact deletion.
7. Select final term shape from correctness, cold-start, size, and incremental-write evidence.
8. Land new schema as a typed rebuild-required transition with compatibility and rollback fixtures.
9. Land prepared, affected-symbol batch maintenance through one SQLite writer.
10. Benchmark rebuild-handle WAL, cache, temp store, page size, sorted load, and bounded workers independently.
11. Remove FTS only after Membrane parity passes, then perform one final Mac rebuild and one final Windows rebuild.
12. Record final DB allocation by table, latency, RSS, WAL, integrity, and seam receipts.

This order attacks measured resolver cardinality first, reuses existing incremental and daemon machinery, avoids two full rebuilds, and keeps schema choice evidence-driven.
