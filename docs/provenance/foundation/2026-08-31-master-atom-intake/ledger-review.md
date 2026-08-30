# Ledger Foundation intake review

## Verdict

**BLOCK archive intake as-is.** All 68 archive rows were evaluated exactly once. Archive taxonomy decomposes current Ledger contracts into implementation slices, acceptance cases, & cross-subsystem behaviors. Result: **47 EXISTING, 16 REGISTER, 2 DUPLICATE, 1 NEW, 2 EXCLUDED, 0 OBSOLETE, 0 UNRESOLVED = 68**.

`NEW` means distinct backlog candidate, not committed scope. No current stable `LDG-*` ID is assigned here; archive key `LDG-MA-053` remains its intake lineage key.

Requested / evaluated / unresolved / excluded: **68 / 68 / 0 / 2**. Archive rows missing from map: **0**. Duplicate archive keys: **0**.

## Frozen scope & freshness

- Current repository/commit: `D:\Claude\membrane` @ `a9a4afb3eeaf4ee00869e8c303c50f810632f273`.
- Archive receipt target: `29adfc8e2fe5a2d43ed25634a91ebec3bb4070d3`. Receipt declares target-commit changes materially invalidating.
- Target-product Ledger source blobs are unchanged between archive target & current commit; `git diff 29adfc..a9a4afb -- engine/crates/membrane-runtime/src/ledger` is empty. Current source therefore revalidates mechanisms individually, but archive receipt remains fingerprint-stale because current canon changed: `LDG-027` & `LDG-028` moved from `EXPLORATORY/HOLD` to `COMMITTED/ADAPT` at `a9a4afb3`.
- Authorized donor evidence was archive matrix only. Donor paths/commits below are carried claims, not independently reopened source.
- Receipt explicitly excluded license disposition. Every donor reuse status is therefore **UNRESOLVED**; treat mechanisms as `REFERENCE_ONLY`, with no direct/translated port authorization until license evidence, obligations, & project policy are recorded.

## Current canonical baseline

Current `docs/canon/ledger.md` contains **27 committed + 1 exploratory capabilities** under one group. Committed lifecycle: **11 DELIVERED, 14 PARTIAL, 2 MISSING; 7 FOCUSED_PASS, 20 PENDING verification; 0 qualification PASS; 16 PUSHED, 11 LOCAL, 0 RELEASED**. Required delivery boundary is `RELEASED`, so **0/27 committed capabilities are closed; 27 open**. Non-counted: **1 group, 28 implementations, 28 qualifications, 8 reference/exclusion decisions**.

Current owner boundary is consistent with `docs/architecture/subsystems/ledger.md`: Ledger owns registered document identity, source-bound structural projection, lexical retrieval, exact resolution, lifecycle, & generated-document projection; Pull/Membrane planner owns final eligibility/fusion/admission; Push owns faithful reduction; Cortex owns durable knowledge; Blueprint owns repository truth; Adapt emits proposals.

## Revised archive evidence counts

Receipt arithmetic reconciles: **408 applicable cells = 101 Observed + 30 Unclear + 277 Not found**. Target-product column reconciles: **68 = 43 Observed + 18 Unclear + 7 Not found**.

Current operative recheck changes target-product evidence to **37 Observed + 25 Unclear + 6 Not found = 68**:

- Downgrade `LDG-MA-016` to `Unclear`: `doc_projection.rs::project_markdown` has no production caller; sync writes only a lexical `DocumentProjectionV1` directly at `doc_spine.rs:730-744`.
- Downgrade `LDG-MA-058` to `Unclear`: archive cell itself admits complete alias-history coverage is partial; current canon `LDG-024` is PARTIAL.
- Downgrade `LDG-MA-064`–`067` to `Unclear`: `session_projection.rs::build_session_projection` & `index_session_projection` have no caller outside their defining module/tests.
- Downgrade `LDG-MA-068` to `Unclear`: `RegisteredDocCandidateProvider` & `select_doc_candidates_for_shadow` have no production caller; module explicitly says shadow-only/future seam.
- Upgrade `LDG-MA-063` to `Observed`: `doc_spine.rs::walk` propagates `read_dir`, entry, & file-type failures before opening transaction; `doc_spine.rs::sync` calls it at 557, then publishes/tombstones only inside transaction at 558–775. Live consumer: `cli.rs:3881`.

## Full 68-row migration map

| Archive ID | Archive behavior | Disposition | Stable target / register | Reason |
|---|---|---|---|---|
| LDG-MA-001 | Canonical source/document identity | EXISTING | LDG-001 | Same registered canonical identity/revision/hash contract. |
| LDG-MA-002 | Confined canonical source references | EXISTING | LDG-026 | Exact WorktreeDocRef admission behavior. |
| LDG-MA-003 | Alias & ambiguous identity resolution | EXISTING | LDG-024 | Alias-history/current-resolution contract; implementation remains partial. |
| LDG-MA-004 | Source snapshot binding | EXISTING | LDG-003 | Persisted hierarchy binds revision & generation; source identity/hash begins at LDG-001. |
| LDG-MA-005 | Document class & lifecycle metadata | EXISTING | LDG-001 | Registration eligibility metadata, not independent outcome. |
| LDG-MA-006 | Trust/influence/sensitivity classification | REGISTER | LDG-I001 → LDG-001 | Policy columns & pre-rank constraint implement source registration/grant. |
| LDG-MA-007 | Permission sync & authorization eligibility | EXCLUDED | Cross-owner decision | External ACL synchronization is not current worktree Ledger scope; source connector updates ACL, planner authorizes admission. |
| LDG-MA-008 | Standards-aware Markdown parsing | EXISTING | LDG-002 | Same GFM/source-position parse contract. |
| LDG-MA-009 | Code-fence-safe headings | DUPLICATE | LDG-MA-008 → LDG-002 | Parser conformance case, not independently observable capability. |
| LDG-MA-010 | Source-positioned spans | EXISTING | LDG-003 | Same persisted span/provenance behavior. |
| LDG-MA-011 | Frontmatter & preamble retention | REGISTER | LDG-I002 → LDG-002 | Parser/projection coverage case. |
| LDG-MA-012 | Ordered section hierarchy | EXISTING | LDG-003 | Exact hierarchy behavior. |
| LDG-MA-013 | Breadcrumb/header-path context | EXISTING | LDG-014 | Heading ancestry retrieval. |
| LDG-MA-014 | Parent/child relationships | EXISTING | LDG-003 | Persisted hierarchy edges. |
| LDG-MA-015 | Previous/next adjacency | EXISTING | LDG-014 | Sibling/document-order retrieval. |
| LDG-MA-016 | Structure-preserving token subdivision | REGISTER | LDG-I020 → LDG-020 | Helper exists but lacks production consumer; projection mechanism/qualification, not new atom. |
| LDG-MA-017 | Content-derived section fingerprint | EXISTING | LDG-004 | Exact stable-section identity contract. |
| LDG-MA-018 | Human aliases & duplicate disambiguation | EXISTING | LDG-004 | Alias side of same identity contract. |
| LDG-MA-019 | Author-defined stable heading IDs | REGISTER | LDG-I004 → LDG-004/LDG-015 | Additional alias/link mechanism; current parser read path has no author-ID mechanism. |
| LDG-MA-020 | Stable derived node/block identity | EXISTING | LDG-004 | Same source/structure-bound node identity. |
| LDG-MA-021 | Exact section/page hydration | EXISTING | LDG-005 | Exact current source-region read. |
| LDG-MA-022 | Freshness revalidation before hydration | DUPLICATE | LDG-MA-021 → LDG-005 | Required condition of exact-current read, sharing caller/failure semantics. |
| LDG-MA-023 | Typed resolution outcomes | EXISTING | LDG-006 | Exact typed-resolution contract; implementation partial. |
| LDG-MA-024 | Continuation/pagination cursors | EXISTING | LDG-005 | Exact-read pagination. |
| LDG-MA-025 | Markdown link extraction | EXISTING | LDG-015 | One syntax tranche of unified link resolution. |
| LDG-MA-026 | Relative path & README/index normalization | EXISTING | LDG-015 | Same unified link resolver contract. |
| LDG-MA-027 | Fragment/heading-anchor resolution | EXISTING | LDG-015 | Same unified link resolver contract. |
| LDG-MA-028 | Anchor-local metadata/subresults | REGISTER | LDG-I011 → LDG-011 | Section-hit result shape from lexical retrieval, not separate capability. |
| LDG-MA-029 | Unicode & case query normalization | EXISTING | LDG-007 | Exact query normalization scope. |
| LDG-MA-030 | Diacritic normalization/original preservation | REGISTER | LDG-I007 → LDG-007 | Tokenizer/display implementation & acceptance case. |
| LDG-MA-031 | CJK/Thai segmentation | EXISTING | LDG-007 | Canon already commits CJK/mixed-script correctness; qualification remains pending. |
| LDG-MA-032 | Language stemming | REGISTER | LDG-Q007 → LDG-007 | Tokenizer experiment/qualification option; archive proves no current need or winner. |
| LDG-MA-033 | Identifier alias extraction | EXISTING | LDG-007 | Exact short-identifier normalization behavior. |
| LDG-MA-034 | Field-separated lexical index | EXISTING | LDG-010 | Exact FTS projection contract. |
| LDG-MA-035 | Heading/title/content weighting | EXISTING | LDG-010 | Weighted field behavior already bundled with FTS projection. |
| LDG-MA-036 | Metadata/filter lanes | REGISTER | LDG-I010 → LDG-010/LDG-001 | Index/filter mechanism supporting pre-rank eligibility. |
| LDG-MA-037 | Positional term postings | REGISTER | LDG-I010 → LDG-010 | FTS implementation detail; archive shows no separate user/operator outcome. |
| LDG-MA-038 | Deterministic lexical ranking | EXISTING | LDG-011 | Exact BM25 generation-bound retrieval. |
| LDG-MA-039 | Short-query exact routing | EXISTING | LDG-008 | Exact canon behavior. |
| LDG-MA-040 | Safe query construction/escaping | EXISTING | LDG-009 | Exact canon behavior. |
| LDG-MA-041 | Scoped inclusion/exclusion/body selection | REGISTER | LDG-I002/LDG-I010 → LDG-001/002/010 | Index configuration & eligibility mechanism. |
| LDG-MA-042 | Structure-first retrieval | EXISTING | LDG-014 | Hierarchy retrieval before bounded expansion; implementation partial. |
| LDG-MA-043 | Targeted hydration after selection | EXISTING | LDG-005 | Same exact-current read contract. |
| LDG-MA-044 | Section summaries/compression | EXCLUDED | Push boundary | Generated compression/reduction belongs Push; source-authored summary remains LDG-001 metadata. |
| LDG-MA-045 | Tree thinning/parent roll-up | REGISTER | LDG-I020 → LDG-020 | Projection shaping mechanism, replay-gated, not separate outcome. |
| LDG-MA-046 | Bounded hierarchy expansion | EXISTING | LDG-016 | Exact bounded expansion behavior. |
| LDG-MA-047 | Dense-child parent promotion | REGISTER | LDG-I016 → LDG-016 | Candidate expansion/ranking policy; Pull still owns final admission. |
| LDG-MA-048 | Adjacency gap fill | REGISTER | LDG-I016 → LDG-016 | Candidate expansion strategy under same caps/provenance. |
| LDG-MA-049 | Bounded link expansion/cycle abstention | EXISTING | LDG-016 | Exact link-backed expansion contract. |
| LDG-MA-050 | Rebuildable projection/index | EXISTING | LDG-018 | Rebuild equivalence contract. |
| LDG-MA-051 | Full rebuild/full load | EXISTING | LDG-018 | Full side of same sync/rebuild contract. |
| LDG-MA-052 | Incremental sync/poll | EXISTING | LDG-018 | Incremental side of same sync/rebuild contract. |
| LDG-MA-053 | Checkpoint & resume | NEW | retain LDG-MA-053 as BACKLOG intake key | Distinct operator-observable interrupted-ingestion recovery; no current Ledger mechanism. |
| LDG-MA-054 | Slim existence enumeration | REGISTER | LDG-I018 → LDG-018 | Scale optimization for authoritative pruning, not separate product behavior. |
| LDG-MA-055 | Transactional generation publication | EXISTING | LDG-017 | Exact atomic-publication contract. |
| LDG-MA-056 | Authoritative tombstoning | EXISTING | LDG-018 | Removal side of authoritative sync. |
| LDG-MA-057 | Explicit source-scoped erase | EXISTING | LDG-019 | Exact erase contract; public operator path remains absent. |
| LDG-MA-058 | Supersession/alias continuity | EXISTING | LDG-024 | Same behavior, currently partial; archive `Observed` is overclaim. |
| LDG-MA-059 | Content-addressed fragment dedup | REGISTER | LDG-I020 → LDG-020 | Storage optimization; canonical source/node identities must remain separate. |
| LDG-MA-060 | Shadow/dual-run migration | EXISTING | LDG-013 | Exact shadow behavior. |
| LDG-MA-061 | Qualification-gated activation | EXISTING | LDG-012 | Exact activation contract. |
| LDG-MA-062 | Schema/tokenizer/normalizer binding | REGISTER | LDG-I012/LDG-Q012 → LDG-012 | Receipt fingerprint & index mechanism, not separate capability. |
| LDG-MA-063 | Failure-safe pruning enumeration | EXISTING | LDG-018 | Required authoritative-sync safeguard; current source proves it despite archive `Not found`. |
| LDG-MA-064 | Document/session separation | EXISTING | LDG-025 | Exact session-vs-document projection boundary; implementation has no live consumer. |
| LDG-MA-065 | Projection derivation/invalidation lineage | EXISTING | LDG-020 | Same deterministic projection/provenance contract. |
| LDG-MA-066 | Projection omissions diagnostics | EXISTING | LDG-020 | Same projection completeness/receipt contract. |
| LDG-MA-067 | Deterministic projection digest/replay | EXISTING | LDG-020 | Same deterministic projection contract. |
| LDG-MA-068 | Source-bound provider candidates | EXISTING | LDG-022 | Candidate materialization boundary; provider seam has no production consumer & stays PARTIAL. |

## NEW candidate dossier

### LDG-MA-053 — checkpoint & resume

- **Scope/state:** distinct `BACKLOG`, not promoted. Owner: Ledger ingestion lifecycle. Boundary: resume an interrupted authoritative scan without duplicate/omitted source items while preserving one atomic published generation.
- **Current operative path:** `engine/crates/membrane-runtime/src/ledger/doc_spine.rs::sync` (`:544`) performs one full walk, opens one SQLite transaction, & commits once; `rg checkpoint|resume` finds no Ledger ingestion mechanism. Live current consumer is `engine/crates/membrane-runtime/src/cli.rs:3881`, which calls `doc_spine::sync`. Therefore implementation=`MISSING`, verification=`PENDING`, qualification=`PENDING`, delivery=`LOCAL`, action=`HOLD` pending scale evidence.
- **Archive donor claim:** Onyx `backend/onyx/connectors/interfaces.py::{CheckpointedConnector,PollConnector}` & `backend/onyx/background/celery/celery_utils.py::_checkpointed_batched_items` @ `cbfd6b327b348beac532801306de63eed8551248`. Donor source was not independently reopened.
- **Reuse/license:** `UNRESOLVED`; receipt excluded license. Permitted disposition is `REFERENCE_ONLY`; no copy/direct/translated port.
- **Acceptance:** kill after each persisted batch; resume from cursor bound to exact repository/source snapshot; no duplicate/omission; changed revision returns typed `rescan_required` & full rebuild; incomplete run never publishes/tombstones; resumed result equals clean rebuild byte-for-byte at projection/generation boundary.
- **Qualification:** failure-injection crash/restart on corpus large enough to justify batching, installed tray-owned daemon path, bounded disk growth/latency, & rollback to full rebuild. If current corpus scale never breaches transaction/latency budget, disposition becomes `OBSOLETE/NOT_APPLICABLE`, not committed scope.

## Highest-value REGISTER candidates

1. **LDG-MA-019 → LDG-004/015:** author-defined stable heading IDs. Current `outline.rs::resolve_section_index` accepts slug/ordinal aliases or span fingerprints only. Archive donor claim: mdBook `crates/mdbook-html/src/html/tree.rs::{build,add_header_links}` @ `dc21064…`. Qualify duplicate IDs, renamed headings, fragment links, Unicode, & source-hash drift. Reuse/license unresolved.
2. **LDG-MA-054 → LDG-018:** slim existence scan. Current `doc_spine.rs::walk` enumerates paths before parsing but has no persisted identity-only cursor/lane. Add only after corpus-scale evidence; complete-scan receipt must gate tombstones.
3. **LDG-MA-047/048 → LDG-016:** dense-child parent promotion & adjacency gap fill. No current operative mechanism. Keep Ledger-local candidate shaping under explicit hop/node/token caps; Pull owns final fusion/admission. Require replay proving lower fragmentation without precision/authority regression.
4. **LDG-MA-016/045 → LDG-020:** structure-preserving subdivision/tree roll-up. `doc_projection.rs::project_markdown` & `cascade_section` implement helper logic, but no production caller exists. Wire/qualification gap, not new atom.

## Cross-subsystem ownership decisions

- `LDG-MA-007`: source/connector owns ACL synchronization; Membrane planner/Pull applies authorization eligibility. Ledger may store a source-bound projection but cannot become authorization authority.
- `LDG-MA-044`: Push owns generated faithful compression. Ledger may register source-authored summaries as metadata, never treat generated summary as exact source truth.
- `LDG-MA-047/048`: Ledger may produce ranked structural candidates; Pull owns cross-provider fusion, sufficiency, attention, & admission.
- `LDG-MA-064`–`067`: document-shaped session artifacts remain Ledger projections; they do not become Cortex durable knowledge without Cortex admission.
- `LDG-MA-068`: Ledger owns candidate materialization/freshness; Pull owns provider invocation/admission. Current seam explicitly does not join planner candidates.
- Repository semantics remain Blueprint; durable knowledge remains Cortex; Adapt may propose tuning but cannot mutate Ledger truth/policy.

## Evidence & canon defects

1. Archive Foundation receipt is stale by its own invalidation rule; it targets `29adfc…`, while requested target is `a9a4afb…` & committed scope changed.
2. Archive excluded license disposition yet recommends donor combinations. Those recommendations cannot become port actions.
3. Seven target `Observed` cells fail live-consumer/complete-behavior gate (`016`, `058`, `064`–`068`).
4. `LDG-MA-063` archive `Not found` misses current fail-safe behavior: enumeration error aborts before transaction/tombstone.
5. `LDG-MA-016` cites `project_markdown` as consumed by sync/session indexing, but no production call exists.
6. `LDG-MA-064`–`067` cite generated-session path as live, but both public functions are uncalled.
7. `LDG-MA-068` calls a future shadow seam a planner-boundary consumer; no consumer exists.
8. Current canon evidence revisions (`f42b6c…` receipts) do not prove current `RELEASED` qualification; all qualification rows remain PENDING/STALE. This correctly yields zero closed capabilities, but prose or archive `Observed` must not imply closure.
9. Archive atomization double-counts parser conformance (`009`) & exact-read freshness (`022`) and moves implementation/qualification mechanics into capability totals. Current normalized canon is stronger at **27 committed behaviors**, not 68.

## Foundation summary

Foundation: Ledger

Count view: CAPABILITY

Capabilities: **0/27 closed; 27 open**

Non-counted: **1 group; 28 implementations; 28 qualifications; 8 reference/exclusion decisions**

Archive intake: **47 EXISTING; 16 REGISTER; 2 DUPLICATE; 1 NEW(BACKLOG); 0 OBSOLETE; 2 EXCLUDED; 0 UNRESOLVED**

Verdict: **BLOCK archive intake as-is; accept migration map & one unpromoted backlog candidate.**
