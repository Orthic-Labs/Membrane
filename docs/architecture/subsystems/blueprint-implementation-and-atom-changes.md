# Blueprint: implementation plan and atom changes

**Date:** 12 September 2026  
**Status:** Implementation specification. No repository changes or runtime benchmarks are claimed by this document.  
**Repository inspected:** `Orthic-Labs/Membrane`, `main` at `bb4f399422e053b74fc094efafad24b7884769d3`.  
**Supersedes:** `Blueprint_Final_Architecture.md` and `Blueprint_Final_Shape_Implementation.md` for refresh/storage design and the atom changes specified here.  
**Objective:** Minimize change/manual-trigger → correct, queryable graph latency without regenerating a healthy graph.

## 1. Decision

Build **one current graph in one SQLite database, maintained by one incremental updater**. Manual refresh, integrated write notifications, watcher events, and reconciliation invoke that same updater.

```text
Manual refresh / integrated write / native watcher / reconciliation
                              │
                  Capture versioned source inputs
                              │
              Reuse or extract file-local fact batches
                              │
                 Diff the affected facts and inputs
                              │
          ┌───────────────────┴─────────────────────┐
          │                                         │
  Direct projection deltas                 Contextual resolution
  file membership, symbols,                explicit dependencies,
  local edges, search postings              memoization, early cutoff
          │                                         │
          └───────────────────┬─────────────────────┘
                              │
                   Prepare an affected-row patch
                              │
        Commit graph + search + provenance + source state together
                              │
             Acknowledge the requested source boundary
```

Use the existing Rust providers and SQLite integration. Use `notify` for external changes, a bounded Rayon pool for independent extraction, and Salsa behind an internal adapter for contextual computations. Simple relational projections use direct row/set deltas; they do not need Salsa. These are implementation choices, not permanent capability requirements.

**Do not build:** a separate facts database, per-generation overlay files, overlay-chain readers, periodic generation compaction, automatic named snapshots, an always-resident duplicate graph, a new generic dependency engine, or an OS service dedicated to this work.

The graph is eager where it matters: required local facts, bindings, graph edges, search entries, and their evidence are ready when refresh reports completion. Optional deep analyses, external semantic enrichment, descriptions, and embeddings do not delay the baseline graph unless explicitly requested by the caller's semantic profile. Required existing semantics must not be quietly relabeled optional to improve timings.

Atoms describe useful behavior. They do not preserve donor storage layouts or algorithms. Section 3 supplies their replacement wording; apply it with the implementation rather than treating older wording as a veto.

## 2. Non-negotiable behavior

### 2.1 No healthy-graph reconstruction

Whole-graph construction is allowed only for **first initialization** or **confirmed corruption**. A separately demonstrated sub-second implementation may earn a future exception; no such exception is enabled now.

Manual refresh, checkout/rebase/stash, a large dirty set, missing runtime caches, watcher failure, restart, configuration changes, and provider upgrades are **not rebuild reasons**. There is no percentage crossover. Large updates change batch size, concurrency, and staging—not the preservation contract.

Enforce this with a private construction entry point requiring `FirstInitialization` or `ConfirmedCorruption`. Reuse an existing equivalent guard where available; do not create a second permission framework. Tests replace the entry point with a trap after initial construction. Log every production invocation and its reason.

| Operation | Policy |
|---|---|
| Reconcile a subtree or repository against indexed source | Allowed when observation evidence requires it; produces changed inputs |
| Extract a changed file or an affected extractor profile | Allowed; reuse unaffected facts |
| Re-evaluate context affected by configuration/provider changes | Allowed; retain equal outputs and write only differences |
| Process a checkout affecting most files | Same resumable delta pipeline; no clear-and-repopulate |
| Create SQL indexes or convert existing schema records | Allowed maintenance; no semantic graph regeneration |
| Create an explicitly requested checkpoint by copying stored data | Allowed; not parsing/building a graph |
| Reconstruct the entire graph under a renamed “incremental” method | Forbidden on a healthy graph |
| Build a full temporary historical graph as a hidden comparison shortcut | Forbidden in the normal comparison/refresh path |

A genuinely global semantic-input change can require broad computation. Report that work honestly; the policy eliminates unnecessary regeneration, not work whose result actually changes.

### 2.2 Completion and failure safety

A completed refresh means the required graph is queryable for its **declared scope, captured inputs, and supported semantic profile**. A dirty mark, queued task, or successful parse alone is not completion.

Prepare before mutating the public graph. Validate the affected patch before commit. A failed preparation or rolled-back transaction leaves the last committed graph intact. Readers see one committed view, not a mixture of generations. This is the useful part of “last-known-good”; it is not a promise to retain every previously committed graph forever.

If a published result is later found to be semantically wrong, correct its affected inputs/results through a delta. Use verified backups or targeted repair for actual corruption. Transaction atomicity does not prove that a resolver is semantically correct.

### 2.3 Ownership and scope

Blueprint owns observation, source identity, providers, resolution, publication, and freshness. Pull requests evidence; Hub and the CodeRight daemon hold the existing resident lifecycle. They do not become alternate graph writers.

Manual operations work when no resident holder exists and leave no resident process behind. When a holder exists, manual and watcher work use the same owner. A timeout after mutation dispatch never authorizes blind replay through another owner.

A published graph remains available during preparation. A caller demanding a newer source boundary may wait or receive a typed timeout/incomplete response; serving the older graph cannot also satisfy that newer boundary.

## 3. Atom changes: exact behavior to implement

The inspected ledger is `docs/canon/blueprint.md`. **BPT-012 is provider execution safety**, not a prohibition on Pull requesting freshness. **BPT-027 requires bounded traversal and rejection of stale cursors**, not permanent storage of their generations. Named snapshots are BPT-036; comparisons are BPT-037. [R1]

### 3.1 Replacement observable-behavior cells

Use the following text as the replacement `Observable behavior` cells. Keep the IDs stable. “Keep” preserves the capability; the text makes the relevant boundary explicit. “Rewrite” changes the contract. “Demote” removes the capability from the core-refresh delivery requirement, not user data already stored.

| ID | Disposition | Replacement observable behavior |
|---|---|---|
| **BPT-002** | Rewrite | Observe captured repository, worktree, configuration and provider inputs; discover additions, deletions and repeated dirty edits relative to Blueprint's indexed state. Ordinary known-path refresh must not require repository-wide discovery. Report incomplete observation explicitly. |
| **BPT-003** | Clarify | Account for every considered source in the authorized inclusion scope, including untracked sources, with an indexed, ignored, unsupported, rejected, failed or pending disposition. Never silently omit considered inputs. |
| **BPT-006** | Rewrite scheduling | Ingest validated SCIP facts with source, build-context, producer and position-encoding identity. Publish external semantic enrichment with its own freshness/coverage; do not block baseline structural refresh or present obsolete enrichment as current. An explicitly requested authoritative profile may wait within its deadline. |
| **BPT-012** | Keep | Execute providers within declared repository access and permissions, with bounded resources, cancellation and explicit failure handling. Provider execution must not mutate source or make undeclared network/process effects. |
| **BPT-013** | Keep | Give repositories, files, entities, occurrences, claims and evidence stable identities appropriate to their semantics; reconcile moves/renames conservatively. Coordinates and the newest publication number must not force unrelated identity changes. |
| **BPT-014** | Clarify | Bind returned evidence to exact source identity/span, provider/profile, provenance, uncertainty and the served view. Reuse shared provenance records; do not rewrite unchanged fact rows merely to copy a new global publication ID. |
| **BPT-015** | **Rewrite** | Prepare and validate graph changes, then atomically publish graph, search, active membership, provenance and source-state updates. Failed preparation or an uncommitted transaction preserves the last committed graph. Readers never see partial publication; unchanged facts are not rewritten. No particular physical generation-storage format is required. |
| **BPT-016** | Keep/rewrite mechanism | Serialize graph publication and maintenance through one repository owner across resident and bounded explicit callers. Enforce deadlines and safe migrations; recover or resume interrupted work without replaying ambiguous mutations or replacing a healthy store. |
| **BPT-019** | Rewrite | Report the source boundary actually covered, observation gaps/pending work, and provider completeness independently. Never label old or unobserved evidence current. A successful requested refresh makes its required graph results queryable before acknowledgement. |
| **BPT-020** | **Rewrite** | Maintain derived results from their actual source, configuration, provider and semantic dependencies, including absent candidates and context. Re-evaluate affected results, replace changed dependency subscriptions, and stop downstream propagation when observed semantic output is unchanged. A new publication ID alone must not invalidate reusable results. |
| **BPT-021** | Strengthen | Demonstrate incremental results against fresh initialization of isolated test graphs for the same captured inputs, including add/remove/rename/config/provider/restart/no-op cases. Enforce zero whole-graph construction on normal production refresh paths, regardless of dirty-set size. |
| **BPT-027** | Clarify | Bound traversal and pagination work. Read each logical operation from one committed view; bind continuation cursors to that view, scope and query. Reject expired or mismatched cursors with a restart response. Do not retain historical generations solely to keep ordinary cursors alive. |
| **BPT-028** | Clarify | Search the current graph through bounded indexed path, identifier, type and text access. Preserve exact, folded, ambiguous and supported substring behavior; report truncation rather than scanning/hydrating the entire graph for a bounded lookup. |
| **BPT-036** | **Demote to optional checkpoint work** | Create, list and read named, explicitly requested self-contained graph checkpoints. Normal refresh neither creates checkpoints nor retains every generation. Preserve existing user checkpoints; failure or absence of checkpoint support must not block live refresh. |
| **BPT-037** | **Demote to optional comparison work** | Compare explicitly selected available checkpoints or supported source baselines without mutating current truth. Distinguish file/content differences from semantic differences. When required historical facts are absent, report that limitation rather than silently constructing full historical graphs. |
| **BPT-042** | Clarify | Expose the same graph/query/refresh semantics through installed resident and bounded explicit adapters. Manual refresh remains functional with no resident holder and invokes the same incremental implementation. |
| **BPT-043** | Rewrite | Run enrolled native observation, incremental refresh and bounded reconciliation while Hub or CodeRight holds the existing shared resident lifecycle. Releasing the final holder stops automatic workers. Report observation failure and preserve pending changes; explicit operations remain available. |
| **BPT-044** | Clarify | Return consistent statuses for complete, degraded, pending, failed, superseded and expired-view outcomes across adapters, mapped to the versioned public envelope. Expose source freshness, coverage and retryability without converting uncertainty to empty success. |
| **BPT-056** | Strengthen | Recover verified corrupt data using the smallest safe repair or verified backup; reconstruct the whole graph only when corruption requires it. Busy, stale, old-schema, unavailable or partially refreshed stores are not corruption. Preserve non-rebuildable user data. |

BPT-007/018 (resolution), BPT-017 (conservative re-anchoring), BPT-023–026 and BPT-029–035 (bounded evidence/navigation), and BPT-065–067 (import/barrel checks) remain acceptance requirements for their supported semantics. They must not be replaced with approximate bare-name matches. Root confinement, trust checks, redaction, document/claim provenance, and unrelated product capabilities remain in force.

### 3.2 Retire these requirements/mechanisms—not their useful outcomes

| Retire from normal refresh | Outcome retained / replacement |
|---|---|
| A newly staged/adopted physical SQLite database for every edit | One affected-row publication transaction |
| Separate generation-overlay files, base chains and overlay compaction | One current transactional graph; self-contained checkpoints only on explicit request |
| Table-wide rewriting of the current generation on every fact | Publication identity at the view boundary; stable fact/provenance identities |
| A global generation parent invalidating every cached result | Actual input dependencies and semantic equality cutoff |
| Root-wide discovery/hash/attestation on every known-path update | Qualified observation evidence, scoped recovery and bounded audits |
| A full graph or all-adjacency reconstruction before bounded queries | Indexed SQL lookup/frontier access |
| Automatic snapshot creation and indefinite ordinary cursor leases | Explicit checkpoints; generation-bound cursors that can expire |
| Always building temporary old/new Git worktrees for semantic comparison | Reuse selected checkpoint facts; report unavailable historical semantics |
| Stop/restart observation for every manual refresh | Serialize writes under the same owner while continuing observation |
| “Large change set,” missing memo, restart or timeout → rebuild | Resumable affected-input preparation |
| A donor's algorithm or comparison label as a shipping prerequisite | Current functional, safety and measured performance acceptance |

Do not remove `atomic_adopt.rs` indiscriminately: it may remain appropriate for first initialization, verified recovery, checkpoint publication, or a separately owned installer/update operation. Remove it from ordinary graph-delta publication. Similarly, do not delete historical user files simply because their automatic generation mechanism is retired.

### 3.3 Apply the ledger changes without inventing states

The inspected validator already accepts `COMMITTED`, `EXPLORATORY`, `BACKLOG`, and `EXCLUDED`; it has no `OPTIONAL` scope or `RETIRED` implementation-state value. [R2]

- Keep rewritten core atoms `COMMITTED`. Mark verification of materially changed behavior `STALE` and qualification `PENDING`; assess implementation as `PARTIAL`/`UNKNOWN` until the replacement is verified. Do not copy an old `FOCUSED_PASS` or `CURRENT_BEST` claim onto changed behavior.
- Move **BPT-036 and BPT-037 to `BACKLOG` for required delivery**, with `Competitive=NOT_COMMITTED`. Their existing explicit surfaces/data remain usable where already safe; the isolated checkpoint adaptation can ship independently. They are not dependencies of core refresh qualification.
- Preserve old acceptance/comparison evidence as historical evidence. New results must use real revision-bound receipts, not invented hashes or declarations of completion.
- Rewrite the relevant implementation rows below. Record retired mechanisms in the existing decision register; do not give an implementation row an unsupported `RETIRED` state. No capability ID is being wholesale deleted by this amendment.
- Run the existing canon generator/validator. `docs/canon/README.md` and the generated pending index must be regenerated, not hand-edited. Do not redesign the entire cross-subsystem ledger to deliver this change. [R2, R3]

| Implementation rows | Required update |
|---|---|
| I002/I003 | Indexed-source observation and inclusion accounting, not tracked-only or every-refresh full scanning |
| I006 | Source/context-qualified external semantic lane |
| I013/I014 | Stable identity and shared evidence, without global-row resealing |
| I015/I016 | Transactional delta publication and shared owner/recovery; historical adoption mechanism removed from the edit path |
| I019/I020 | Explicit source proof plus keyed dependencies and output cutoff |
| I021 | Fresh-initialization oracle plus production no-rebuild trap; replace “post-crash rebuild” as a normal recovery expectation |
| I027/I028 | Transaction-pinned indexed reads, bounded cursors and explicit expiration |
| I036/I037 | Optional checkpoint/comparison implementations, no refresh prerequisite or hidden treeish full builder |
| I042/I043/I044 | Shared manual/watcher updater and consistent bounded status/lifecycle handling |
| I056 | Verified scoped repair before corruption-authorized reconstruction |

The prefixes in this table are `BPT-`. These are replacements for existing records, not new capability counts. All code, generated docs and tests that enforce the old mechanism must change together.

## 4. Runtime contracts and the single update path

Keep the coordinator inside `membrane-blueprint`. Reuse existing store, root authorization, provider, cancellation and lifecycle code. New internal modules are justified only for responsibilities that cannot remain readable in those existing modules.

Logical interfaces below define behavior, not a new public protocol or assumed Salsa macro syntax:

```rust
struct RefreshRequest {
    scope: AuthorizedScope,
    target: RefreshTarget,       // ExplicitPaths or Repository
    boundary: SourceRequirement,
    mutation_id: Option<MutationId>,
    deadline: Deadline,
}

struct CapturedBatch {
    id: BatchId,
    base_revision: GraphRevision,
    observation_epoch: ObservationEpoch,
    inputs: Vec<CapturedInput>,  // Exact bytes/key or deletion/disposition
    covered_work: CoveredWork,
    source_proof: SourceProof,
}

struct PreparedPatch {
    batch: BatchId,
    base_revision: GraphRevision,
    input_preconditions: Vec<InputVersion>,
    row_deltas: Vec<TypedRowDelta>,
    computation_updates: Vec<ComputationResult>,
    coverage: Coverage,
}

fn capture(req: &RefreshRequest, owner: &RepoOwner) -> Result<CapturedBatch>;
fn prepare(batch: &CapturedBatch, owner: &mut RepoOwner) -> Result<PreparedPatch>;
fn publish(patch: PreparedPatch, owner: &mut RepoOwner) -> Result<RefreshResult>;
```

`TypedRowDelta` is a closed internal enum. Do not accept table names or arbitrary SQL from providers. Each write is scoped to provider/owner-supported records.

The end-to-end procedure is:

1. Resolve authorization and the canonical repository owner. A bounded explicit caller attaches to an available owner or acquires its bounded lease; it does not create a competing writer.
2. Establish the requested source boundary. Use supplied paths/mutation evidence or discovery appropriate to observation continuity.
3. Capture stable source/configuration inputs. Hash the bytes actually supplied to extraction. Represent absence, rejection, unsupported content and instability explicitly.
4. Load or extract local batches; diff them against active batches for the affected file/profile owners.
5. Apply simple projection changes to an unpublished candidate view. The view is the committed base plus affected overrides/tombstones, not a whole-graph clone and not a durable overlay-generation system.
6. Select and evaluate required contextual results against that same candidate view. Keep iterating affected computations until their results/coverage are established or the request must return incomplete.
7. Validate the patch: source/profile preconditions, ownership, occurrence bounds, supported relation targets, required coverage, and dependency records. Validation must scale with affected data on the normal path.
8. In one short publication transaction, verify the base, apply affected row changes, update graph/search/provenance and source state, and acknowledge covered work. Parsing and semantic evaluation stay outside this transaction.
9. Commit, then notify waiters. A source-identity no-op may acknowledge observation progress without creating another graph revision or rewriting facts.

Manual requests bypass coalescing delay and wait for publication. Watcher batches run the same path automatically. “Synchronous manual” describes caller completion, not a ban on parallel independent parsing.

For large changes, persist bounded preparation chunks in the same database. Readers continue using the preceding publication until the complete required patch commits. Final application can take longer than a small edit; neither time pressure nor staging pressure permits a full rebuild. An atomic multi-file mutation is not split into separately acknowledged incomplete graph states.

## 5. Storage implementation

### 5.1 One database, different kinds of records

Use `.agent/graph/graph.db`. Its ordinary `-wal`/`-shm` files are SQLite's journal mechanism, not application generation overlays. WAL is already enabled in the inspected store. [R4]

The public graph tables are **mutable inside transactions**. Cached local fact batches are immutable payloads but can be garbage-collected when unreferenced. Append-only history for every edge is not required.

| Logical relation | Key and content | Reuse/change |
|---|---|---|
| Publication manifest | Singleton current graph revision/ID, source identity, profile, coverage and counts | Adapt existing `generation`; update once per actual graph publication |
| Active file/profile | Path identity + provider/extraction profile → source digest, active batch or disposition, context | Adapt file-state/owner tables; no global revision rewrite |
| Local fact batches | Batch key → extractor manifest, payload, coverage and byte accounting | Immutable cache in this same database |
| Current graph facts | Stable node/edge/occurrence IDs, values and ownership/support | Adapt existing files/symbols/edges/fact-owner tables |
| Semantic inputs | Stable contextual key → value/digest | Changed keys only; missing/empty values are explicit |
| Computation results | Stable operation key → semantic/evidence result, provider identity and coverage | Published derived results, not serialized Salsa internals |
| Computation uses | Consumer → input/child computation key and observed semantic/evidence version | Reverse-indexed for work selection; replaced after execution |
| Search projections | Exact/folded names, identifier terms, FTS rows | Updated with their graph owners |
| Pending source/preparation | Epoch, dirty paths/subtrees, batch/base, captured identities, stage/cursor | Durable recovery data, not an unbounded event archive |
| Optional checkpoint catalog | Name, self-contained file, captured graph identity, status | Explicit operations only; never consulted by ordinary graph queries |

Do not duplicate an existing relation just to use these names. First produce a column/index mapping, then add only missing fields/tables. Preserve provider extension fields, user claims, documents, external annotations and their source identities.

### 5.2 Reference schema for missing incremental metadata

This SQL is an executable **reference schema for isolated tests**, not a migration to run blindly against the existing database. Map it to existing tables where equivalent storage already exists. Production identifiers remain versioned and compatible with existing public IDs.

```sql
PRAGMA foreign_keys = ON;

CREATE TABLE IF NOT EXISTS refresh_batches (
    batch_id TEXT PRIMARY KEY,
    base_revision INTEGER NOT NULL,
    observation_epoch TEXT NOT NULL,
    state TEXT NOT NULL CHECK (state IN
      ('capturing','extracting','evaluating','prepared','committing',
       'published','pending','failed','cancelled')),
    captured_manifest BLOB,
    coverage BLOB,
    created_ms INTEGER NOT NULL
);

CREATE TABLE IF NOT EXISTS local_fact_batches (
    batch_key TEXT PRIMARY KEY,
    source_digest TEXT NOT NULL,
    extractor_key TEXT NOT NULL,
    parse_inputs_digest TEXT NOT NULL,
    payload BLOB NOT NULL,
    payload_bytes INTEGER NOT NULL CHECK (payload_bytes >= 0),
    last_used_ms INTEGER NOT NULL
);

CREATE TABLE IF NOT EXISTS active_file_profiles (
    path_key BLOB NOT NULL,
    profile_key TEXT NOT NULL,
    display_path TEXT NOT NULL,
    source_digest TEXT,
    batch_key TEXT REFERENCES local_fact_batches(batch_key),
    disposition TEXT NOT NULL,
    context_digest TEXT NOT NULL,
    PRIMARY KEY (path_key, profile_key)
);

CREATE TABLE IF NOT EXISTS semantic_inputs (
    input_key TEXT PRIMARY KEY,
    semantic_digest TEXT NOT NULL,
    value BLOB NOT NULL
);

CREATE TABLE IF NOT EXISTS semantic_results (
    computation_key TEXT PRIMARY KEY,
    evaluator_key TEXT NOT NULL,
    semantic_digest TEXT NOT NULL,
    evidence_digest TEXT NOT NULL,
    result BLOB NOT NULL,
    coverage BLOB NOT NULL
);

CREATE TABLE IF NOT EXISTS computation_uses (
    consumer_key TEXT NOT NULL
      REFERENCES semantic_results(computation_key) ON DELETE CASCADE,
    dependency_kind TEXT NOT NULL CHECK (dependency_kind IN ('input','query')),
    dependency_key TEXT NOT NULL,
    aspect TEXT NOT NULL CHECK (aspect IN ('semantic','evidence')),
    observed_digest TEXT NOT NULL,
    PRIMARY KEY (consumer_key, dependency_kind, dependency_key, aspect)
);
CREATE INDEX IF NOT EXISTS computation_users
    ON computation_uses(dependency_kind, dependency_key, aspect, consumer_key);

CREATE TABLE IF NOT EXISTS prepared_patch_chunks (
    batch_id TEXT NOT NULL REFERENCES refresh_batches(batch_id) ON DELETE CASCADE,
    ordinal INTEGER NOT NULL,
    payload BLOB NOT NULL,
    payload_digest TEXT NOT NULL,
    PRIMARY KEY (batch_id, ordinal)
);
```

The polymorphic dependency target is checked by the typed evaluator and patch validator; a single SQL foreign key cannot point to two possible target tables. Missing candidates are represented as existing semantic-input keys with empty/missing values, not dangling unvalidated references.

Store path identity losslessly. Do not universally lowercase Windows paths, convert non-UTF-8 paths lossily into primary keys, or assume a display string uniquely identifies a filesystem object. Preserve the existing confinement policy and qualify case-only renames on each supported filesystem.

### 5.3 Publication transaction and no-op writes

```text
BEGIN IMMEDIATE
    verify base graph revision, owner and captured provider/input versions
    make referenced cached batches available
    retract affected owner/support relationships and obsolete occurrences
    insert/update affected current file, symbol and relationship records
    replace changed contextual results and their dependency uses
    update affected exact/term/FTS records and source-bound evidence
    apply count differences and affected content-identity leaves
    update manifest and coverage
    acknowledge only covered source work; finish the preparation record
COMMIT
notify callers
```

Use equality-guarded updates rather than unconditional upserts:

```sql
-- Pattern only: map to the existing fact columns and digest representation.
INSERT INTO current_fact(fact_id, payload_digest, payload)
VALUES (?1, ?2, ?3)
ON CONFLICT(fact_id) DO UPDATE SET
    payload_digest = excluded.payload_digest,
    payload = excluded.payload
WHERE current_fact.payload_digest <> excluded.payload_digest;
```

Rows with multiple supporting owners are removed only when their last valid support is removed. Preserve unresolved/ambiguous reference occurrences rather than dropping them to satisfy endpoint constraints. Apply dependency-related row order or deferred constraints deliberately.

Separate graph revision from content identity, last actual row change, and observation progress. A global publication change is not a reason to change every fact ID or invalidate every computation. Keep existing source/content-root calculation only where consumed, and update it incrementally. Do not replace table-wide writes with table-wide manifest serialization or recounting.

### 5.4 Database settings

Start with `journal_mode=WAL`, `foreign_keys=ON`, and **`synchronous=FULL` on connections that acknowledge durable work**. Keep prepared statements and bounded connections. Record the actual SQLite runtime/build and use a maintained release with the documented WAL-reset correction or official backport. Do not assume a wrapper-crate version establishes the embedded SQLite version. [S1]

Use deadline-aware busy handling and statement cancellation. Query and writer connections are separate; do not run an interleaved reader over its own connection's partial writes. Keep read transactions short and measure checkpoint delays/WAL growth. Do not disable automatic checkpointing without a bounded tested replacement. Never force a truncating checkpoint on every save.

`FULL` is the chosen initial acknowledgement policy, not a claim that faulty hardware honors flushes. Platform durability behavior belongs in installed tests. A future weaker policy must say so explicitly; it cannot benchmark away durability while retaining the old completion claim.

## 6. Ingestion, capture and reconciliation

### 6.1 Triggers

| Entrance | Behavior |
|---|---|
| Integrated writer | Submit committed mutation ID, affected paths and optional bytes/edit ranges. Reuse supplied bytes only when the trusted write protocol establishes their identity. Duplicate watcher events become no-ops. |
| Watcher | Enqueue bounded hints. Initial settings: 20 ms quiet window, 100 ms maximum selection delay from the oldest pending event. Do not reset the maximum indefinitely. |
| Manual, explicit paths | Flush relevant pending work, capture immediately, update semantic consequences and wait. Do not claim unrelated source was inspected. |
| Manual, repository | Establish qualified observation continuity or reconcile against indexed state, then run the same updater. |
| No resident holder | Open the installed bounded owner, use durable facts, reconcile the requested scope, publish and exit. No leaked watcher or daemon. |

The timings are initial tuning values, not measured performance claims. Start independent extraction with at most two workers across the shared pool, bounded by available parallelism. Raise limits only when measured multi-file benefit outweighs contention with compilation and other repositories.

Callbacks must not parse, recurse through directories, block on SQL or wait for disk flushes. Maintain a bounded queue and an uncertainty flag that survives losing detailed events: on overflow, mark the affected root/subtree uncertain. The owner persists dirty work. A crash before persistence creates an observation gap on restart, not a false successful acknowledgement.

`notify` has documented backend/editor limitations. Handle errors, unsupported watching, dropped-event indications and root deletion explicitly. Watchman-style synchronized clocks are a useful reference, but `notify` does not automatically supply an equivalent source barrier. [S4, S10]

### 6.2 Stable source capture

Read exact bytes, hash that buffer and pass the same buffer to extraction. In the stable case, read it once. Before/after identity and metadata checks plus bounded retries detect concurrent replacements/writes; unresolved instability remains pending or unknown. No metadata heuristic proves an atomic snapshot of arbitrary hostile concurrent writes.

Handle deletion and atomic-save replacement distinctly. Retry Windows sharing violations within the remaining deadline; never translate permission/read failure into deletion. Directory renames produce grouped old/new membership changes and may reuse content-keyed local facts.

Apply consistent inclusion rules at observation, reconciliation and extraction. Ignore changes themselves must be observed. Do not rely on “ignore at attach” where the backend still observes an entire subtree; filter early and qualify backend behavior. No automatic edits to Git hooks, global Git settings or OS watcher limits are part of this delivery.

Use backend-supported notification settings; do not blindly force a 1 MiB Windows buffer. Microsoft documents zero-byte overflow behavior and a 64 KiB buffer limit for network monitoring. Local graph databases remain on supported local storage. [S9]

### 6.3 Recovery without reconstruction

Choose the smallest justified uncertainty scope: known paths, subtree or entire repository. Compare membership, dispositions and exact source identity against the indexed manifest. Include new/untracked directories, disappeared files and newly unignored content.

Metadata mismatch and racy timestamps prioritize hashing. They do not eliminate exact-content checks when observation is unknown. Same-size/same-mtime changes are a required test. [S5]

Keep two mechanisms:

- **On-demand recovery:** startup gaps, watcher failure/overflow and strict manual requests trigger deadline-bound reconciliation. Preserve its cursor and uncertainty when interrupted.
- **Rolling resident audit:** revisit membership and content, including metadata-unchanged files, in bounded low-priority chunks. Initial cap: 8 MiB/s content reads and one audit worker; yield to foreground updates. Record audit coverage/age rather than promising that a fixed timer means everything is current.

A full source inventory or hash pass is allowed when necessary. Its output is a delta; it does not reparse unchanged files or reset graph tables. Under continually changing source, report the captured observation interval and gaps rather than inventing an instantaneous global snapshot.

Start observation before baseline acquisition, then reconcile overlapping events before declaring continuity. A source callback counter is not proof that notifications for all completed external writes have arrived. Strict freshness uses a qualified cooperative/snapshot/barrier mechanism or honest bounded reconciliation.

### 6.4 Git and buffers

Git is an accelerator. When the remembered HEAD changes, candidate discovery can combine the old/new commit diff, previously dirty paths, currently dirty paths and current untracked membership. Still compare against Blueprint's active source state: status text does not distinguish every successive edit of an already-dirty file. [S11]

Blob OIDs are cache hints unless equivalence with parsed working-tree bytes is established. Attributes, line endings, filters, `ident`, encodings and extraction context all matter. Do not use a blob's coordinates as raw working-tree coordinates without a correct mapping. [S12]

Unsaved editor buffers are outside this implementation. Future support must use a caller/session-scoped buffer view over the disk graph, never one global “open buffer is truth” override shared by unrelated agents.

## 7. File facts, identity and parsing

Compute the extractor identity once per release/profile:

```text
extractor_key = H(canonical manifest of:
    grammar/runtime compatibility + external scanner,
    extraction query assets or traversal code,
    analysis profile + implementation/fact-schema version)

local_batch_key = H(
    exact_source_digest,
    extractor_key,
    relevant_parsing_input_digest)
```

Tagged public digests retain their established meaning. New internal identifiers can have their own versioned encoding. Do not rehash grammar binaries on every file; do not hide independently varying inputs behind a hand-maintained version string. Resolver-only configuration belongs to contextual computation, not a repository-wide parse-cache salt.

A local batch contains declarations, lexical scopes, references, imports, alias/re-export syntax, source occurrences, proven local edges, diagnostics and extraction coverage. Extractors that depend on path/context must declare that input or separate that work; path independence is not assumed.

| Case | Work |
|---|---|
| Same active bytes/profile | Skip extraction and unnecessary fact writes; process changed context and source-progress bookkeeping |
| Undo returns to a cached historical version | Reuse its batch, diff against the currently active version, update projections/bindings |
| Same bytes move to another path | Reuse eligible local facts, change path/module membership and location-sensitive semantics |
| Comment/formatter changes bytes | Normally extract again; keep semantic propagation stopped where outputs remain equal |
| Resolver configuration changes | Reuse local facts; evaluate affected contextual computations |
| Extractor/grammar changes | Resumable affected-profile extraction; preserve unaffected profiles and compare actual outputs |

A cache hit saves extraction; it does not mean the active graph is already correct. Detect payload/key inconsistencies as corruption rather than silently accepting mismatched cache content.

Initially use full changed-file parsing and drop the tree. Give each worker its own mutable parser. Retained-tree optimization requires a measured parsing bottleneck, a bounded hot set, exact edit coordinates, and whole-file differential conformance for the particular grammar/runtime. Fragment-only re-parsing is not a correctness oracle. [S13, S14]

Keep semantic identity separate from occurrences. Inserting a comment may preserve a binding while moving its source span. Update evidence without propagating a false semantic change. Avoid duplicating target declaration coordinates into every incoming edge when source-bound occurrence references can serve the same public response.

When parsing fails, either publish trustworthy partial facts with degraded coverage and the new byte identity, or keep the previous publication pending under the provider's failure policy. Do not quietly substitute the old batch into a current complete graph. Last-good evidence remains explicitly stale and source-bound.

## 8. Semantic maintenance: direct deltas where sufficient, dependencies where necessary

### 8.1 Direct projections

Diff and update owner-scoped file membership, declarations, lexical containment, local relationships, identifier terms and search records directly. Batch SQL operations are appropriate. A raw name match is only candidate selection, not a cross-file semantic edge.

Maintain support/ownership for facts derived from more than one input. Deleting one owner must not erase another owner's valid result. Retain empty/missing candidate values so newly created files can invalidate lookups that previously relied on their absence.

### 8.2 Contextual computations

Use typed, fine-grained operations such as:

```text
export_bucket(module, name, namespace, build_context)
scope_candidates(scope, name, namespace, build_context)
module_candidates(importer, specifier, build_context)
alias_target(alias, build_context)
resolve_reference(reference, build_context)
required_graph_unit(unit, semantic_profile)
```

Do not use one repository-wide map as the input to every query. Register actual configuration fields/scopes, directory membership/search rules, negative probes, supported macro/build inputs and relevant semantic signatures. A file-level exported-surface hash may accelerate a proven equality boundary; it cannot replace these dependencies.

Salsa is the initial evaluation engine because it supplies tracked computation and output-equality cutoff. Blueprint invokes the required operations during refresh; ordinary Pull does not pay for their unfinished evaluation. The atom requires the behavior, not this crate forever. [S3]

### 8.3 Work selection and propagation

Persist reverse-indexed dependency uses from stable input/child-operation keys to materialized consumers. Discovery postings cover new declarations/references and previously absent candidates. They select work; they are not a separate handwritten memo-validation engine.

```text
changed captured inputs
    → changed keyed local/context values
    → direct subscribed consumers and discovery-selected operations
    → evaluate against the common post-batch state
    → replace dependency uses, even if semantic output is unchanged
    → if output changed, visit its subscribed consumers
    → if only evidence changed, update evidence consumers only
    → stop when required affected outputs are validated
```

Deduplicate by operation and relevant input state, not “once per file.” Consumers reuse their stored references. Reparse a consumer only when its declared extraction inputs actually changed, not merely because its import target changed.

For `import { run as execute }`, the call named `execute` depends on an alias operation that depends on `run`. A selection limited to `changed_names = {run}` must follow that relationship; merely calling a correct resolver for the wrong subset is still incorrect. Scope membership and configuration-only changes require the same care.

When a provider cannot prove narrower selection, widen to its declared module/package/context using stored facts and report the cost. Do not silently skip affected work or fall back to reconstructing the repository.

### 8.4 Cycles and concurrent files

Extract changed files independently, then resolve against the shared candidate input state. No file-import topological waves are needed for local parsing.

Semantic cycles do need explicit provider behavior. Where justified, evaluate the affected strongly connected semantic component to a finite monotone fixed point; handle unsupported/non-converging cases explicitly. For deletions, retract obsolete support rather than allowing a cycle's old positive facts to justify themselves. Never publish intermediate iterations as a complete graph. Salsa's documented cycle facilities require appropriate semantics; their availability is not a convergence proof. [S6]

### 8.5 One provenance model, without depending on Salsa internals

For each materialized computation persist its stable key, evaluator identity, semantic result digest, evidence digest, supported output references, direct dependency uses and coverage. Use those records for work selection and the dependency portion of receipts.

Implement provider-side typed read helpers that record input reads and child-operation calls. On a child memo hit, record the child's stable semantic identity/digest; the child already owns its own dependency record. Do not assume access to undocumented internal Salsa dependency IDs.

Keep dependency metadata/evidence separate from semantic equality. A changed trace must be saved even when output is equal, without making every parent observe it as a semantic change. Tests must cover branch changes, missing-to-present candidates, reused children and cache hits.

Persisted graph/results/subscriptions survive restart. Salsa memos do not have to. Hydrate only requested/affected computations from durable facts; a cold memo cache is not permission to touch every graph node.

## 9. Queries, source evidence and completeness

A query first satisfies any requested source boundary through the existing Blueprint owner, then opens its read transaction. Do not hold that transaction while waiting for refresh. The remaining request deadline covers both phases; joining a shared refresh does not let a single timed-out waiter cancel work still needed by others.

```text
BEGIN (read transaction)
    read publication manifest and observation state
    validate cursor/scope/query identity
    run indexed symbol/search/frontier queries
    assemble source-bound evidence and coverage
COMMIT / release read transaction
```

SQLite supplies a stable view during that read transaction. Ordinary pagination does not keep the transaction open between requests. A continuation cursor carries graph revision, query/options hash, scope/authorization binding and stable sort position. If the current revision differs, return an explicit expired/restart outcome rather than combining versions. [S2]

Required indexes include exact path/stable ID, exact/folded symbol name with scope/context, references by owner and name/specifier, outgoing and incoming edges by kind, owner/support lookup, and dependency-key → consumer lookup. Check `EXPLAIN QUERY PLAN`; bound expansion in the query itself. No all-graph hydration or edge scan to build temporary adjacency for a small query.

Use exact matching first, precomputed camelCase/snake_case component terms where useful, and FTS5 trigram search for supported infix queries. For one/two-character infix searches, use explicit small n-gram postings with candidate verification and limits, or return a documented unsupported mode; do not silently replace existing substring behavior with exact-only matching. Built-in FTS5 trigram matching has short-query limitations. A custom C tokenizer is not a prerequisite. [S7]

Register per-provider/per-operation coverage: included source scope, candidate enumeration, aliases/re-exports, supported dispatch/macros/generated code, and meaning of unresolved/ambiguous/unsupported. Complete callers require current classifications for all relevant supported call occurrences, not only previously queried references. Traversal ceilings are a separate completeness limitation.

The response keeps these distinctions:

```text
view: graph revision + source/profile identity
source: covered boundary, observed/applied progress, gaps and pending scope
coverage: supported complete / degraded / incomplete / unsupported
provenance: source spans, provider identity, dependency references
omissions: unavailable, excluded, ambiguous, deadline or output limits
enrichment: SCIP/other producer state and its own source/context identity
```

A read-set is not proof that the observer saw all external changes. `partial_ok` is not `stale_ok`. An exact unresolved-candidate count is reported only after enumeration establishes that count; otherwise return unknown or a lower bound.

Pinned graph evidence must refer to stored/captured bytes or a separately verified source read matching its digest. Never combine old graph coordinates with the current disk contents without verification.

SCIP enrichment runs independently, is coalesced/cancellable, and publishes through the same writer when its result is qualified against the source/build context it analyzed. Whole-context dependencies may invalidate its authority without requiring row-by-row rewrites; selection must filter stale enrichment. A newer core generation must not be overwritten by a delayed external producer. No untrusted repository build/indexer is launched without the existing provider permissions.

## 10. Optional checkpoints and comparisons

### 10.1 Checkpoints do not participate in refresh

BPT-036 becomes an explicit operation. It copies already-published data; it never re-extracts the repository or creates an overlay chain. Use SQLite's supported online backup mechanism or an explicitly scoped consistent export. [S8]

Implementation sequence:

1. Validate name, authorization, destination and requested source/view policy. Reuse the existing snapshot API where practical; do not add a second snapshot service.
2. Capture a consistent stored view into a temporary self-contained checkpoint. For an exact requested revision, pin a supported read view or serialize publication for this explicit operation. Do not assume an incrementally progressing backup still represents its original start revision after source changes.
3. Verify the completed checkpoint's actual manifest/schema/identity. Store that observed identity in the catalog; never label the copy with an unverified requested revision.
4. Finish/close its SQLite state so the checkpoint does not depend on the live graph's WAL or cached batches. Seal the output and then register it as available.
5. A crash between sealing and catalog registration can leave an orphan checkpoint; recover/reconcile that record explicitly. It cannot damage or change the live graph.

Copying may be repository-sized and may exceed one second. This is the cost of an explicit checkpoint, not a graph reconstruction and not part of edit latency. Its cancellation/deadline policy is explicit. Do not copy only a live database's main file while ignoring WAL contents.

Checkpoints have their own retention policy and are not silently garbage-collected as cache entries. Existing named snapshots must remain preserved through migration. During cutover, unsupported legacy formats receive a truthful migration/read error, not deletion or empty success.

### 10.2 Comparisons

BPT-037 compares available checkpoint facts or another supported baseline. A Git file diff can identify changed paths/content, but must not be labeled a complete semantic graph diff without the needed facts.

The inspected `lib_application_snapshots.rs` includes a `treeish_semantic_graph` route that materializes a detached worktree and invokes the full graph builder. Remove that route from ordinary `changes`/refresh processing. A missing historical baseline is not permission to fabricate a fresh temporary-store “first initialization” exception. [R8]

Reuse stored historical fact batches where their context is qualified; otherwise report unavailable/partial historical semantics. A future dedicated historical-analysis feature needs an explicit scoped contract and cost, not a hidden side effect in a fast graph query. Neither current graph contents nor source files are overwritten by comparison.

## 11. Failure handling, retention and migration

### 11.1 Failure matrix

| Event | Required behavior |
|---|---|
| Source changes while being captured | Bounded recapture or pending/unstable status; no claim to exact current bytes |
| Provider returns malformed facts/crashes | Reject its invalid output; retain prior committed graph or publish qualified degradation |
| Newer work commits before an older prepared patch | Reject/rebase affected preparation; never overwrite newer state |
| Disk full, store busy, permission failure | Deadline-bound typed failure; not corruption, not rebuild permission |
| Crash during preparation | Resume compatible captured work, reuse batches, preserve public graph |
| Crash before publication commit | Previous committed publication remains authoritative |
| Crash after commit before reply | Query batch/mutation status; do not replay an ambiguously dispatched mutation |
| Client waiter cancels | Detach that waiter; do not cancel shared work required by other callers |
| No resident holder and request ends | Close the bounded owner; saved pending work waits for an explicit/resident continuation, not a leaked worker |
| Watcher loses continuity | Mark uncertainty, reconcile changed inputs, do not clear the graph |
| Missing/corrupt individual rebuildable batch or index | Verify and repair the affected data/projection where possible |
| Confirmed unrepairable graph/database corruption | Preserve diagnostics/non-rebuildable data, restore verified backup or use recorded corruption construction permission |

Do not identify arbitrary schema mismatch, stale source, timeouts or inability to open a file as confirmed corruption. Do not manufacture an absent graph by deleting a healthy one.

### 11.2 Bounded retention from the first cache release

Protect active file/profile batches, active computation/provenance records and in-flight/resumable preparation. Self-contained checkpoints protect their own content and do not require retaining the live database's historical cache indefinitely.

Start with a 512 MiB historical-cache budget per repository, excluding protected active data. Evict unreferenced least-recently-used historical batches down to 80% of the cap. This is an initial policy, not a claimed optimal memory footprint. Never evict active truth to satisfy the historical-cache limit.

Reclaim abandoned preparation only after its ownership/status is resolved. Bound runtime memo entries and reuse stable semantic keys rather than allocating a permanent input for every edit. Retain enough mutation completion metadata for the supported idempotency/status window; after expiry, return unknown/expired rather than pretending a retry is known not to have executed.

Run reclamation in small transactions under the owner. Do not execute full FTS rebuilding, VACUUM or semantic graph regeneration on every startup/eviction. Keep SQLite maintenance independent of ordinary graph refresh.

Recent-query prewarming is **deferred**, not a core freshness prerequisite. If later enabled, use bounded, deduplicated optional query roots with preserved authorization, cancel superseded work, and never replay arbitrary old read-sets or compete with core publication.

### 11.3 Existing-store migration

1. Inventory actual columns, queries, provider extensions, snapshot formats and active call sites. Read current repository instructions and fetch current `main` before editing; this file is pinned evidence, not an assumption that the repository stopped changing.
2. Acquire the canonical owner/lease and make a consistent rollback backup. Quiesce incompatible writers; retain observation where supported.
3. Add missing incremental metadata/indexes. Backfill ownership and existing source identities from stored records, not by rebuilding the source graph.
4. Migrate readers and writers together away from filtering every fact by the latest `generation_id`. Keep public response identities coherent while changing physical storage.
5. Initialize missing local-fact/dependency metadata for affected files or through resumable targeted backfill. Keep conservative correct selection until narrower provider adapters are qualified. Do not throw away valid graph data because the new evaluator is cold.
6. Preserve existing named checkpoints and route their explicit operations separately. Disable/remove automatic historical full-builder routes before the no-rebuild guard is considered closed.
7. Run shadow comparisons only in isolated test/temporary stores without duplicating production effects. There is one production writer.
8. Switch behavior and corresponding atoms/docs/tests in the same release change. Old binaries must not write an incompatible schema. Rollback restores the compatible binary/schema backup or uses a tested reverse conversion; it does not reset the live graph.

A database copy for explicit migration backup is allowed. Per-edit database copying is retired. Retain the old implementation only during a bounded migration window and delete production-reachable legacy refresh selectors after acceptance.

## 12. Code changes and delivery order

### 12.1 Source map

Paths below are within `engine/crates/membrane-blueprint/src/` unless stated otherwise. Existing seams were inspected in this conversation at the pinned revision; proposed test/module names below are not claims that they already exist.

| Location | Change |
|---|---|
| `engine.rs` | Route all refresh entrances into capture/prepare/publish; remove ordinary repository discovery and broad bidirectional `affected_reference_closure`; enforce construction guard |
| `delta_store.rs` | Remove twelve-table generation propagation, repeated full recounts and per-file reseal work; apply one affected batch transaction |
| `store.rs` / migrations | Coherent read transactions, additive metadata/indexes, immutable-batch cache, durable writer settings, backup and recovery integration |
| `atomic_adopt.rs` | No use for ordinary delta refresh; retain only justified first-init/recovery/checkpoint or separately owned update consumers |
| `query.rs` / `graph.rs` query seams | Indexed lookup/frontier access; retain ambiguity, unresolved edges, limits and cursor validation; remove all-graph hydration for bounded operations |
| `watch.rs` | Native callbacks, bounded coalescing, persisted dirty scopes, source continuity and delta-producing reconciliation |
| `freshness.rs` / `freshness_receipt.rs` | Exact source identity, observation/coverage separation, source barriers and qualified enrichment; eliminate mandatory root scan on known-path refresh |
| `identity.rs` and existing identity helpers | Stable fact/entity identities; distinguish occurrence changes from semantic changes; no global generation salt on reusable inputs |
| `module_resolution.rs`, existing provider/static-provider code | Split local extraction from contextual evaluation; add typed dependency reads, coverage and targeted selection |
| `lib_application_snapshots.rs` | Optional self-contained checkpoint adapters; retire hidden temporary-worktree graph construction from normal comparison |
| `lib_operations_repair.rs` | Verified scoped recovery; no generic-error healthy-store reset |
| `service.rs` / CLI / native adapters | One updater, deadline and cancellation semantics, resident/bounded parity, atomic publication status |
| `docs/canon/blueprint.md` | Apply Section 3 replacements and implementation/qualification/decision-record updates |
| `docs/architecture/subsystems/blueprint.md` | Replace physical immutable-generation prescription with observable atomicity; separate checkpoints from live refresh |
| `docs/architecture/execution-lifecycle-boundary.md` | Amend full-scan attestation, watcher stop/restart handoff, and pre-edit closure requirements to this implementation |
| `scripts/ci/check-atomic-canons.mjs` and generated indexes | Use existing valid schema; regenerate outputs and keep evidence truthful, rather than inventing enum states |

The checked lifecycle document currently prescribes scan/handoff/repair details that conflict with the new mechanism. Amend those clauses with replacement tests; do not simultaneously retain the old guarantee and delete its implementation. [R9]

### 12.2 Phases

| Phase | Deliverable | Acceptance before progressing |
|---|---|---|
| **P0 — Guard and contracts** | Construction guard, scoped atom amendments, baseline trace, disposable initial-build oracle | All healthy-store entry points trap full construction; no fake changed-contract PASS states |
| **P1 — Storage/query fixes** | Atomic affected-row writes, no global generation rewrites, indexed reads, expired cursor behavior | Unrelated rows untouched; rollback/coherence tests; indexed query plans |
| **P2 — Manual delta and cache** | Captured-input batches, local fact cache with basic GC, synchronous explicit-path refresh | Same-byte no-op, undo, rename, source spans, timeout/restart correct |
| **P3 — Required semantics** | Direct projections, tracked contextual adapter, dependency provenance and coverage | Alias/shadowing/re-export/config/cycle deletion tests; no dependent-source reparse unless its inputs changed |
| **P4 — Automatic freshness** | Watcher, bounded batching, direct write input, source continuity, recovery/audit | Source edits become visible without query-triggered repair; automatic/manual parity; no reset on gaps |
| **P5 — Broad changes and cutover** | Resumable large deltas, migration/rollback, explicit checkpoint isolation, external enrichment | Checkout/profile upgrade/crash/low-disk tests; hidden historical builder removed; no steady-state memory/disk leak |
| **P6 — Measured optimization only** | More workers, hot trees, specialized operators or resident cache only where justified | Improvement at equal semantics/durability; no additional current-truth owner |

Do not block P1/P2 on completing every provider's Salsa adapter. Retain the current correct provider semantics behind the new coordinator while replacing its affected-work selection incrementally. A provider that cannot yet meet the new correctness contract is not marked complete; neither a whole rebuild nor silently reduced semantic coverage is an acceptable shortcut.

The optional new checkpoint/comparison implementation is not a prerequisite to core refresh. The prerequisites are **isolation/preservation of existing checkpoint data** and removal of any prohibited builder dependency.

### 12.3 Development commands and evidence

Run the repository's required build wrapper; the following use the existing RightKit convention and should be reconciled with current `AGENTS.md` before execution:

```sh
rightkit cargo test --manifest-path engine/Cargo.toml -p membrane-blueprint --locked
rightkit cargo build --manifest-path engine/Cargo.toml -p membrane-blueprint --locked
node scripts/ci/check-atomic-canons.mjs --write
node scripts/ci/check-atomic-canons.mjs
```

The Node command is existing development-time documentation validation, not a new production runtime dependency. Add the new regression/performance tests to the crate's actual test targets; do not report these commands as executed by this document.

Record starting/ending SHA, modified source/atoms, test commands, results, hardware/runtime and any residual failures. No additional approval board, donor comparison loop or new “closure system” is required. Old comparison evidence is historical, not proof that a newly changed implementation is best or finished.

## 13. Acceptance tests

Initialize fresh **disposable test graphs** from captured fixtures and compare them with incrementally maintained graphs. Canonical facts, binding outcomes, source spans, support sets, search and semantic coverage must agree. Provenance certificates must be valid, but two evaluators need not produce byte-identical execution traces. Add independent semantic fixtures because both paths can share a resolver bug.

Persist seeds, captured bytes/configuration, event sequences, controlled interleavings and failure injection points for deterministic replay. Test installed Windows and macOS independently, with and without resident holders.

| Test family / suggested test name | Required assertion |
|---|---|
| `healthy_refresh_never_constructs_whole_graph` | Manual, watcher, no-op, restart, checkout, config/profile upgrade and >5% changes never invoke full construction |
| `checkpoint_compare_never_builds_hidden_treeish` | Missing historical semantics yields a qualified limitation, not temporary full graph builds |
| `incremental_keeps_unrelated_rows` | Unchanged owners/facts are not rewritten or rehashed globally for publication |
| `duplicate_event_and_same_bytes_are_noop` | No extraction/fact writes; covered observation progress still advances |
| `cached_undo_reactivates_batch` | B → cached A replaces B's projections and repairs context; cache hit does not terminate processing |
| `comment_edit_updates_occurrences_only` | Correct source spans and evidence without false downstream semantic propagation |
| `alias_reexport_shadowing_config` | Alias callers, barrel chains, wildcard/negative probes and configuration-only changes remain correct |
| `cyclic_delete_retracts_support` | Removed cycle inputs cannot retain self-justifying stale derived facts |
| `multi_file_same_candidate_state` | Parallel extraction resolves against one post-batch state; stale jobs cannot overwrite later commits |
| `same_metadata_missed_event_reconciles` | Exact audit/strict recovery detects metadata-preserving edits and new untracked membership |
| `atomic_save_case_rename_directory_move` | Correct old/new ownership and byte identity across supported path semantics |
| `publication_is_one_read_view` | Concurrent graph/FTS/provenance reads are old or new, never mixed |
| `failed_refresh_preserves_committed_graph` | Provider/validation/SQL failure preserves a usable preceding publication |
| `commit_reply_loss_is_idempotent` | Status resolves a committed mutation without duplicate effects |
| `interrupted_preparation_resumes` | Compatible batches/chunks reused after restart; no fresh whole-graph pass |
| `cursor_expires_without_history_retention` | A stale continuation returns restart; no overlay, archive or long-lived lease is created |
| `source_and_enrichment_freshness_are_separate` | Old SCIP cannot outrank current structural evidence; late enrichment cannot overwrite newer source |
| `manual_works_without_resident_holder` | Same semantics, authorized effects and bounded exit; no process left behind |
| `checkpoint_is_explicit_and_self_contained` | Normal saves create zero snapshot files; explicit checkpoint survives cache GC and represents its recorded view |
| `snapshot_migration_preserves_user_data` | Existing checkpoints survive schema/cutover; unsupported formats are not deleted |
| `cache_gc_and_slow_reader_bounds` | Active/preparation facts protected; historical cache, runtime memos and WAL remain bounded |
| `providers_stay_within_permissions` | Source confinement, cancellation and undeclared side-effect protections survive refactor |

Before dropping any legacy test, classify the behavior it protected. Replace assertions requiring physical database adoption or automatic historical retention with the revised outcome tests. Keep atomicity, safety, ambiguity and no-stale-success assertions.

## 14. Performance and the sub-second exception

Measure **change/trigger → required graph publication**, **first complete query after edit**, and **cold bounded manual refresh** separately. Include observation delay, queue wait, source discovery, hash/extraction, semantic work selection/evaluation, patch preparation, commit/checkpoint effects and response serialization.

Record p50/p95/p99/maximum, CPU, peak memory, bytes read/hashed, parsed files, evaluated/reused computations, changed/written rows, staging size and WAL growth. Use identical semantics, durability and inclusion scopes for alternatives.

Initial engineering targets on recorded reference hardware—not measured results:

| Workload | Target |
|---|---|
| Stable explicit-path file ≤64 KiB, unchanged external semantics, warm manual owner | p95 ≤100 ms to durable required publication |
| Same edit through qualified watcher | p95 ≤150 ms including batching/observation |
| Bounded exact symbol/path query | p95 ≤5 ms inside Blueprint |
| Bounded two-hop traversal, ≤1,000 returned edges | p95 ≤25 ms inside Blueprint |

Test fixed local changes/fanout on 1k/10k/100k-file fixtures and real repositories. Hard scaling assertions are zero unrelated source scan on the known-path hot update, zero unrelated fact rewrites, no all-graph clone, and no full-builder call. Global changes may legitimately take longer; report their actual work and responsiveness.

Optional optimizations require a measured dominant cost: hot trees for extraction, additional workers for independent multi-file load, direct specialized operators for expensive relational results, or a structurally shared resident query cache for a missed SQL query target. A future resident cache must have one coherent generation root and cannot become a second truth owner. No overlay chains are introduced merely to speed pointer swaps.

**Under-one-second full reconstruction remains disabled.** To qualify it later, demonstrate complete user-visible discovery/computation/construction, durable publication and a complete verification query under 1,000 ms within a declared real repository/provider/platform envelope. Include cold/warm conditions the feature claims to support, normal load, repeated runs and maxima. No work may be hidden in prewarming, lazy Pull or asynchronous persistence. Preserve the old valid graph until the candidate is validated; an overrun must not erase it or silently remain enabled. A timing guess or a small no-op fixture is not qualification.

## 15. Completion criterion

Deliver when manual and watcher refresh update required evidence correctly, healthy graphs are preserved, unrelated work is not repeated, queries see coherent indexed data, recovery/GC are bounded, and measured results plus the updated atoms/tests agree.

Do not reopen architecture merely because a donor uses another mechanism. Conversely, do not preserve a mechanism merely because an atom once named it.

**The implementation is one database, one updater, actual input dependencies, affected-row publication, explicit optional checkpoints, and tests that catch both stale results and unnecessary reconstruction.**

---

## Sources and evidence scope

The atom changes and engineering defaults are decisions for this implementation. Source links establish the inspected behavior or library contracts, not benchmark results. Repository links are pinned to the verified head. No application code was changed and no application tests were run to produce this document.

### Repository

- **[R1]** [Blueprint capability and implementation ledger](https://github.com/Orthic-Labs/Membrane/blob/bb4f399422e053b74fc094efafad24b7884769d3/docs/canon/blueprint.md).
- **[R2]** [Canon validator and accepted state values](https://github.com/Orthic-Labs/Membrane/blob/bb4f399422e053b74fc094efafad24b7884769d3/scripts/ci/check-atomic-canons.mjs).
- **[R3]** [Generated canon inventory and register schemas](https://github.com/Orthic-Labs/Membrane/blob/bb4f399422e053b74fc094efafad24b7884769d3/docs/canon/README.md).
- **[R4]** [SQLite store configuration](https://github.com/Orthic-Labs/Membrane/blob/bb4f399422e053b74fc094efafad24b7884769d3/engine/crates/membrane-blueprint/src/store.rs).
- **[R5]** [Refresh engine and dependency closure](https://github.com/Orthic-Labs/Membrane/blob/bb4f399422e053b74fc094efafad24b7884769d3/engine/crates/membrane-blueprint/src/engine.rs).
- **[R6]** [Delta writes and generation propagation](https://github.com/Orthic-Labs/Membrane/blob/bb4f399422e053b74fc094efafad24b7884769d3/engine/crates/membrane-blueprint/src/delta_store.rs).
- **[R7]** [Physical file adoption](https://github.com/Orthic-Labs/Membrane/blob/bb4f399422e053b74fc094efafad24b7884769d3/engine/crates/membrane-blueprint/src/atomic_adopt.rs).
- **[R8]** [Snapshots and treeish semantic construction](https://github.com/Orthic-Labs/Membrane/blob/bb4f399422e053b74fc094efafad24b7884769d3/engine/crates/membrane-blueprint/src/lib_application_snapshots.rs).
- **[R9]** [Execution, lifecycle and freshness requirements to amend](https://github.com/Orthic-Labs/Membrane/blob/bb4f399422e053b74fc094efafad24b7884769d3/docs/architecture/execution-lifecycle-boundary.md).
- **[R10]** [Current query access patterns](https://github.com/Orthic-Labs/Membrane/blob/bb4f399422e053b74fc094efafad24b7884769d3/engine/crates/membrane-blueprint/src/query.rs).

### Primary implementation documentation

- **[S1]** [SQLite WAL, durability, checkpointing and WAL-reset correction](https://www.sqlite.org/wal.html).
- **[S2]** [SQLite isolation](https://www.sqlite.org/isolation.html).
- **[S3]** [Salsa dependency validation and early cutoff](https://salsa-rs.github.io/salsa/reference/algorithm.html).
- **[S4]** [notify backend and editor limitations](https://docs.rs/notify/latest/notify/).
- **[S5]** [Git's metadata race and content checks](https://git-scm.com/docs/racy-git).
- **[S6]** [Salsa cycle handling](https://salsa-rs.github.io/salsa/cycles.html).
- **[S7]** [SQLite FTS5, including trigram and content-table behavior](https://www.sqlite.org/fts5.html).
- **[S8]** [SQLite online backup API](https://www.sqlite.org/backup.html).
- **[S9]** [Windows ReadDirectoryChangesW](https://learn.microsoft.com/en-us/windows/win32/api/winbase/nf-winbase-readdirectorychangesw).
- **[S10]** [Watchman synchronized clock](https://facebook.github.io/watchman/docs/cmd/clock).
- **[S11]** [Git status comparison semantics](https://git-scm.com/docs/git-status).
- **[S12]** [Git attributes and working-tree transformations](https://git-scm.com/docs/gitattributes).
- **[S13]** [Tree-sitter incremental edit API](https://tree-sitter.github.io/tree-sitter/using-parsers/3-advanced-parsing.html).
- **[S14]** [Tree-sitter edit fuzzing](https://tree-sitter.github.io/tree-sitter/cli/fuzz.html).
