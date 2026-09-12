# Membrane decisions — 2026-09-12

**Decision maker:** Adrian
**Status:** accepted architecture; implementation acceptance remains separate

1. Blueprint performs complete construction only when no graph exists or Blueprint proves an existing graph is corrupt & unrecoverable.
2. A newer or incompatible graph is preserved, migrated when possible or rejected with a typed error. It is not treated as corruption.
3. Every update to a valid graph is incremental.
4. Incremental updates run automatically from Hub/CodeRight watcher events or explicitly when an agent executes a user-requested build or refresh.
5. Incremental maintenance combines CodeGraph's event-path, scan-and-diff, hash-confirmation & affected-reference repair with GitNexus's no-change exit & content-addressed reuse.
6. Blueprint context, query, recall, status & freshness reads never construct or update a graph.
7. Membrane is exposed automatically to Codex & Claude Code through their native `SessionStart` hook.
8. `SessionStart` supplies one bounded Membrane orientation packet. It never dumps complete Blueprint, Cortex or Ledger stores.
9. Host invokes Membrane & inserts returned context; one Membrane planner decides evidence selection across all subsystems.
10. Cortex owns durable semantic knowledge admission, lifecycle, storage & retrieval for admitted knowledge, including Adapt Taste & Insights.
11. Ledger owns Markdown/document truth as source-bound indexed projections in Ledger SQLite. Source documents remain authoritative; Ledger's hash-bound resolver returns exact source spans. Ordinary Ledger sections & index projections are not copied into Cortex.
12. Ledger remains a direct Pull provider. Pull may receive relevant Ledger document evidence through the native Ledger provider, with source identity, revision, span hash & resolver ticket preserved.
13. Adapt returns to its August design: offline/manual mining produces reviewed/admitted Taste & Insight knowledge, then writes it through Cortex durable admission. Adapt remains an internal proposal producer, not a direct Pull/context provider.
14. Remove direct Adapt candidate injection/relabeling from federation. Preserve operator/debug mining surfaces & Cortex provenance/types; agents receive admitted Adapt outputs through Cortex.
15. Blueprint keeps its complete graph separately. Selected repository knowledge may enter Cortex with provenance to Blueprint generation & source; live repository evidence remains Blueprint-owned.
16. Ordinary agent-facing context enters through Pull: Cortex knowledge, Blueprint live evidence, direct Ledger document evidence & exact source reads. Pull owns retrieval, selection, budget fitting, faithful reduction, publication & exact recovery.
17. Push as the current context-reduction subsystem is retired. Its selection, truncation, skeletonization, compression, protected-span preservation, spill/externalization, resolver & recovery capabilities move under Pull delivery. Host tool-output capture stays a host/CodeRight integration concern; Membrane does not claim control of Codex transcript compaction.
18. `push` is reserved as the agent-to-Membrane write operation. Initially it accepts durable memory writes only & routes them to Cortex; later typed write destinations require separate accepted decisions. Blueprint changes arise from repository edits, Ledger changes arise from document edits & Adapt remains an internal mining pipeline.
19. A pushed memory preserves the agent-submitted body byte-for-byte in an immutable source/admission record. Cortex may add embeddings, metadata, scope, authority, lifecycle state & derived representations, but none replaces the original body. A user-directed memory write is stored as a memory rather than mislabeled as a pending proposal; Cortex still owns storage integrity, provenance, conflict & lifecycle behavior.

## Historical decisions preserved

Adapt history records the design path that this decision corrects: `190e952c` created/renamed Adapt & consolidated transcript mining; `8bf6bd63` imported Adapt into Membrane with `/v1/memories:batch`; `601c234c` moved persistence ownership to Cortex; `0bceb850` introduced the later drift that prepared Adapt candidates directly for Pull.

Markdown Spine was established as source-reference infrastructure, not memory: `b9220792` created separate `doc_artifacts` source-reference records that were explicitly never memories; `c8f964c8` added direct hash-bound recall pointers so agents resolve exact spans against their original Markdown source.

Memory history predates the proposal-only public surface. At the initial Membrane consolidation in `e7158989`, Cortex/MemRight already exposed direct `try_put`/`put` storage: `try_put` assigned `content.to_string()` to the memory row & persisted that content in SQLite. The later public MCP shape in `b9220792` replaced raw put with `membrane_knowledge_propose`, whose first implementation returned `needs_review` without persisting. This decision restores an agent write path while retaining Cortex ownership & exact submitted-source preservation.

Push/Pull terminology entered the product in `4a31f285`: Push compressed active work, instructions & session state; Pull retrieved context; Persist retained knowledge. That model assumed Membrane could manage host context. Codex owns its transcript & compaction, while current Push primarily reduces Membrane-selected packets. Keeping that work as a peer subsystem no longer matches its actual boundary.
