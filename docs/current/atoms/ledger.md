# Ledger atomic capability canon

Normalized from pre-standardization worktree canon inventory based on `d84322c3df182ff1d6ef7ca96fe94aea22273894`. Required delivery boundary: `RELEASED`.

Only committed capability rows count. Implementation, verification, qualification, delivery & evidence remain independent; closure is derived.

## Group register

| ID | Parent | Owner | Scope | Derived rollup |
|---|---|---|---|---|
| LDG-G01 | — | Ledger | COMMITTED | 22 committed capabilities; 1 exploratory capability; closure derived from child rows |

## Capability ledger

| ID | Parent | Owner | Scope | Observable behavior | Implementation | Verification | Qualification | Delivery | Action | Evidence |
|---|---|---|---|---|---|---|---|---|---|---|
| LDG-001 | LDG-G01 | Ledger | COMMITTED | Register eligible Markdown sources with canonical identity, revision, content hash, & grant. | PARTIAL | PENDING | PENDING | LOCAL | REPAIR_WIRE | PENDING |
| LDG-002 | LDG-G01 | Ledger | COMMITTED | Parse each revision once into source-positioned GFM structure spanning headings, prose, code, lists, quotes, tables, HTML, links, & nested blocks. | PARTIAL | PENDING | PENDING | LOCAL | REPAIR_WIRE | PENDING |
| LDG-003 | LDG-G01 | Ledger | COMMITTED | Persist ordered document/section/block hierarchy with spans, search text, links, revision, & generation. | PARTIAL | PENDING | PENDING | LOCAL | REPAIR_WIRE | PENDING |
| LDG-004 | LDG-G01 | Ledger | COMMITTED | Derive stable section identity from body span hash + structural context while keeping slug/ordinal as aliases. | UNKNOWN | PENDING | STALE | LOCAL | RECONCILE_EVIDENCE | PENDING |
| LDG-005 | LDG-G01 | Ledger | COMMITTED | Read exact current section only when document hash, anchor, & span verify; paginate by continuation cursor. | UNKNOWN | PENDING | PENDING | LOCAL | RECONCILE_EVIDENCE | PENDING |
| LDG-006 | LDG-G01 | Ledger | COMMITTED | Return typed current/relocated/stale/missing/ineligible/unsupported results & preserve alias history. | UNKNOWN | PENDING | STALE | LOCAL | RECONCILE_EVIDENCE | PENDING |
| LDG-007 | LDG-G01 | Ledger | COMMITTED | Normalize Unicode, case, punctuation, paths, CJK, mixed scripts, & short identifiers without erasing nonempty queries. | UNKNOWN | PENDING | PENDING | LOCAL | RECONCILE_EVIDENCE | PENDING |
| LDG-008 | LDG-G01 | Ledger | COMMITTED | Route short queries through exact path/title/anchor/identifier before FTS & bounded structural expansion. | PARTIAL | PENDING | STALE | LOCAL | REPAIR_WIRE | PENDING |
| LDG-009 | LDG-G01 | Ledger | COMMITTED | Escape all user terms through one safe FTS builder & expose normalized lane in receipt. | UNKNOWN | PENDING | PENDING | LOCAL | RECONCILE_EVIDENCE | PENDING |
| LDG-010 | LDG-G01 | Ledger | COMMITTED | Build separate rebuildable Ledger FTS5 projection with weighted path/title/heading/body/identifier fields. | UNKNOWN | PENDING | PENDING | LOCAL | RECONCILE_EVIDENCE | PENDING |
| LDG-011 | LDG-G01 | Ledger | COMMITTED | Execute deterministic BM25 section retrieval when `ledger_fts` is active & bind result to generation/source. | UNKNOWN | PENDING | PENDING | LOCAL | RECONCILE_EVIDENCE | PENDING |
| LDG-012 | LDG-G01 | Ledger | COMMITTED | Activate/rollback `legacy_scan`, `shadow`, or `ledger_fts` only with trusted receipt/content address. | UNKNOWN | PENDING | PENDING | LOCAL | RECONCILE_EVIDENCE | PENDING |
| LDG-013 | LDG-G01 | Ledger | COMMITTED | Shadow Ledger & legacy retrieval on same corpus without changing active result. | UNKNOWN | PENDING | PENDING | LOCAL | RECONCILE_EVIDENCE | PENDING |
| LDG-014 | LDG-G01 | Ledger | COMMITTED | Retrieve parent, child, sibling, & heading ancestry from persisted hierarchy. | PARTIAL | PENDING | PENDING | LOCAL | REPAIR_WIRE | PENDING |
| LDG-015 | LDG-G01 | Ledger | COMMITTED | Resolve inline/reference/autolink/image/relative/fragment/Unicode/broken Markdown links consistently. | PARTIAL | PENDING | PENDING | LOCAL | REPAIR_WIRE | PENDING |
| LDG-016 | LDG-G01 | Ledger | COMMITTED | Expand only strong seeds under hop/node/edge caps, cycle detection, provenance, & typed abstention. | PARTIAL | PENDING | PENDING | LOCAL | REPAIR_WIRE | PENDING |
| LDG-017 | LDG-G01 | Ledger | COMMITTED | Publish one transactional generation so readers never observe mixed nodes, links, FTS, or artifacts. | UNKNOWN | PENDING | PENDING | LOCAL | RECONCILE_EVIDENCE | PENDING |
| LDG-018 | LDG-G01 | Ledger | COMMITTED | Incrementally sync changes, tombstone removals, & rebuild an equivalent projection. | UNKNOWN | PENDING | PENDING | LOCAL | RECONCILE_EVIDENCE | PENDING |
| LDG-019 | LDG-G01 | Ledger | COMMITTED | Erase every Ledger-owned projection for granted source identity. | PARTIAL | PENDING | PENDING | LOCAL | REPAIR_WIRE | PENDING |
| LDG-020 | LDG-G01 | Ledger | COMMITTED | Build deterministic document/session projection with provenance, invalidation, cursor, digest, omissions, & content hash. | UNKNOWN | PENDING | PENDING | LOCAL | RECONCILE_EVIDENCE | PENDING |
| LDG-021 | LDG-G01 | Ledger | COMMITTED | Keep virtual session projection non-recallable until consumer, authority, privacy, lifecycle, & replay value qualify. | UNKNOWN | PENDING | PENDING | LOCAL | RECONCILE_EVIDENCE | PENDING |
| LDG-022 | LDG-G01 | Ledger | COMMITTED | Materialize source-bound Ledger section candidates with generation, provenance & freshness for an authorized Pull provider. | PARTIAL | PENDING | PENDING | LOCAL | REPAIR_WIRE | PENDING |
| LDG-023 | LDG-G01 | Ledger | EXPLORATORY | Emit smallest coherent structural evidence/deltas with exact Ledger refs for governed semantic compilation. | MISSING | PENDING | PENDING | LOCAL | HOLD | PENDING |

## Implementation register

| ID | Capability targets | Mechanism | Source/donor | Reuse mode | State | Production consumer |
|---|---|---|---|---|---|---|
| LDG-I001 | LDG-001 | `engine/crates/membrane-runtime/src/ledger/db.rs`; `doc_projection.rs`; indexing tests | Legacy pre-normalization row ledger.md:7 (worktree base d84322c3df182ff1d6ef7ca96fe94aea22273894); dependencies: MEM-004; canon: LDC §3; note: Whole-corpus completeness unproven. | ADAPT | PARTIAL | Ledger sync/Pull |
| LDG-I002 | LDG-002 | `engine/crates/membrane-runtime/src/ledger/outline.rs`; parser/projection tests | Legacy pre-normalization row ledger.md:8 (worktree base d84322c3df182ff1d6ef7ca96fe94aea22273894); dependencies: LDG-001; canon: LDC §7.1; note: Typed block breadth needs full production reconciliation. | ADAPT | PARTIAL | Indexer |
| LDG-I003 | LDG-003 | `engine/crates/membrane-runtime/src/ledger/db.rs`; `doc_spine.rs`; indexing tests | Legacy pre-normalization row ledger.md:9 (worktree base d84322c3df182ff1d6ef7ca96fe94aea22273894); dependencies: LDG-002; canon: LDC §7.2; note: Some block/link fields remain incomplete. | ADAPT | PARTIAL | Recall/resolve |
| LDG-I004 | LDG-004 | `outline.rs::span_anchor`; `index.rs::section_fingerprint`; identity tests | Legacy pre-normalization row ledger.md:10 (worktree base d84322c3df182ff1d6ef7ca96fe94aea22273894); dependencies: LDG-003; canon: LDC §7.3; MPI §18; note: Existing qualification predates identity change; rerun required. | ADAPT | UNKNOWN | Source read/retrieval |
| LDG-I005 | LDG-005 | `outline.rs::read_section_with_cursor`; source-read tests | Legacy pre-normalization row ledger.md:11 (worktree base d84322c3df182ff1d6ef7ca96fe94aea22273894); dependencies: MEM-004, LDG-004; canon: LDC §7.4, §13; note: No whole-document substitution. | ADAPT | UNKNOWN | MCP `membrane_source_read`/CLI |
| LDG-I006 | LDG-006 | `doc_spine.rs`; resolver tests | Legacy pre-normalization row ledger.md:12 (worktree base d84322c3df182ff1d6ef7ca96fe94aea22273894); dependencies: LDG-004–LDG-005; canon: LDC §7.3–7.4; note: Frozen retrieval rerun open. | ADAPT | UNKNOWN | Pull/Cortex source refs |
| LDG-I007 | LDG-007 | `engine/crates/membrane-runtime/src/ledger/index.rs`; `ledger-metrics.json` | Legacy pre-normalization row ledger.md:13 (worktree base d84322c3df182ff1d6ef7ca96fe94aea22273894); dependencies: —; canon: LDC §8.1–8.2; note: MPI requests rerun after later changes. | ADAPT | UNKNOWN | Document recall |
| LDG-I008 | LDG-008 | `engine/crates/membrane-runtime/src/ledger/index.rs`; eval tests | Legacy pre-normalization row ledger.md:14 (worktree base d84322c3df182ff1d6ef7ca96fe94aea22273894); dependencies: LDG-007; canon: LDC §8.3; note: Short-query weakness remains. | ADAPT | PARTIAL | Document recall |
| LDG-I009 | LDG-009 | `engine/crates/membrane-runtime/src/ledger/index.rs`; hostile-query tests | Legacy pre-normalization row ledger.md:15 (worktree base d84322c3df182ff1d6ef7ca96fe94aea22273894); dependencies: LDG-007; canon: LDC §8.4; note: Debug receipt completeness uncertain. | ADAPT | UNKNOWN | Document recall |
| LDG-I010 | LDG-010 | `engine/crates/membrane-runtime/src/ledger/db.rs`; `index.rs`; `ledger-metrics.json` | Legacy pre-normalization row ledger.md:16 (worktree base d84322c3df182ff1d6ef7ca96fe94aea22273894); dependencies: LDG-003; canon: LDC §9.1–9.3; note: Never reuses Cortex/Blueprint DB. | ADAPT | UNKNOWN | Sync/recall |
| LDG-I011 | LDG-011 | `doc_spine.rs::recall`; `ledger-metrics.json` | Legacy pre-normalization row ledger.md:17 (worktree base d84322c3df182ff1d6ef7ca96fe94aea22273894); dependencies: LDG-010; canon: LDC §9.4, §16; note: Qualified lane is not default provider activation. | ADAPT | UNKNOWN | Pull document lane |
| LDG-I012 | LDG-012 | `index.rs::{recall_mode,activate}`; activation receipt test | Legacy pre-normalization row ledger.md:18 (worktree base d84322c3df182ff1d6ef7ca96fe94aea22273894); dependencies: LDG-011; canon: LDC §9.4, §16–17; note: DB default remains `legacy_scan`. | ADAPT | UNKNOWN | Operator/host rollout |
| LDG-I013 | LDG-013 | `doc_shadow.rs`; eval harness; `ledger-metrics.json` | Legacy pre-normalization row ledger.md:19 (worktree base d84322c3df182ff1d6ef7ca96fe94aea22273894); dependencies: LDG-011–LDG-012; canon: LDC §12, §17; note: Negative no-answer baseline predates Pull abstention. | ADAPT | UNKNOWN | Qualification owner |
| LDG-I014 | LDG-014 | `outline.rs`; `doc_spine.rs`; tests | Legacy pre-normalization row ledger.md:20 (worktree base d84322c3df182ff1d6ef7ca96fe94aea22273894); dependencies: LDG-003; canon: LDC §10.1; note: Production expansion breadth unproven. | ADAPT | PARTIAL | Pull structural expansion |
| LDG-I015 | LDG-015 | projection/outline parsers | Legacy pre-normalization row ledger.md:21 (worktree base d84322c3df182ff1d6ef7ca96fe94aea22273894); dependencies: LDG-002–LDG-003; canon: LDC §10.2; note: Persisted unified link graph not proven. | ADAPT | PARTIAL | Link graph/validation |
| LDG-I016 | LDG-016 | `doc_candidate_provider.rs`; spine tests | Legacy pre-normalization row ledger.md:22 (worktree base d84322c3df182ff1d6ef7ca96fe94aea22273894); dependencies: LDG-014–LDG-015; canon: LDC §10.3; note: Link-backed expansion incomplete. | ADAPT | PARTIAL | Pull document lane |
| LDG-I017 | LDG-017 | `engine/crates/membrane-runtime/src/ledger/db.rs`; equivalence tests | Legacy pre-normalization row ledger.md:23 (worktree base d84322c3df182ff1d6ef7ca96fe94aea22273894); dependencies: LDG-003, LDG-010; canon: LDC §4, §13; note: Installed crash proof absent. | ADAPT | UNKNOWN | All Ledger readers |
| LDG-I018 | LDG-018 | `doc_spine.rs::sync`; indexing/equivalence tests | Legacy pre-normalization row ledger.md:24 (worktree base d84322c3df182ff1d6ef7ca96fe94aea22273894); dependencies: LDG-017; canon: LDC §3, §13; note: Randomized full-vs-incremental receipt absent. | ADAPT | UNKNOWN | Daemon/operator |
| LDG-I019 | LDG-019 | Ledger DB methods/tests | Legacy pre-normalization row ledger.md:25 (worktree base d84322c3df182ff1d6ef7ca96fe94aea22273894); dependencies: MEM-004, LDG-017; canon: LDC §13, §19; note: No public erasure surface. | ADAPT | PARTIAL | Future authorized operator |
| LDG-I020 | LDG-020 | `doc_projection.rs`; `session_projection.rs`; tests | Legacy pre-normalization row ledger.md:26 (worktree base d84322c3df182ff1d6ef7ca96fe94aea22273894); dependencies: MEM-022; canon: LDC §14; note: Projection is not document truth. | ADAPT | UNKNOWN | Hub/human navigation |
| LDG-I021 | LDG-021 | session projection eligibility tests | Legacy pre-normalization row ledger.md:27 (worktree base d84322c3df182ff1d6ef7ca96fe94aea22273894); dependencies: LDG-020; canon: LDC §14.3; note: Required negative behavior; enablement remains blocked. | ADAPT | UNKNOWN | Pull/Adapt |
| LDG-I022 | LDG-022 | `doc_candidate_provider.rs`; Pull/Ledger tests | Legacy pre-normalization row ledger.md:28 (worktree base d84322c3df182ff1d6ef7ca96fe94aea22273894); dependencies: LDG-011–LDG-013, PUL-003; canon: LDC §15–16; XSG §9; note: Provider flag separately unpromoted. | ADAPT | PARTIAL | Pull federation |
| LDG-I023 | LDG-023 | No production wire type | Legacy pre-normalization row ledger.md:29 (worktree base d84322c3df182ff1d6ef7ca96fe94aea22273894); dependencies: LDG-003–LDG-006; frozen `docs/pending/semantic-blueprint-review-pack-v2/01-LEDGER-SEMANTIC-PRODUCER-AMENDMENT.md@d84322c3df182ff1d6ef7ca96fe94aea22273894`; canon: LDC structure ownership; note: Compiler/orchestration absent. | ADAPT | MISSING | Future Cortex semantic producer |

## Qualification ledger

| ID | Capability targets | Acceptance boundary | State | Evidence | Material revision |
|---|---|---|---|---|---|
| LDG-Q001 | LDG-001 | Reconcile legacy focused claim through exact live consumer at RELEASED boundary | PENDING | PENDING | LOCAL |
| LDG-Q002 | LDG-002 | Reconcile legacy qualified claim through exact live consumer at RELEASED boundary | PENDING | PENDING | LOCAL |
| LDG-Q003 | LDG-003 | Reconcile legacy focused claim through exact live consumer at RELEASED boundary | PENDING | PENDING | LOCAL |
| LDG-Q004 | LDG-004 | Reconcile legacy stale claim through exact live consumer at RELEASED boundary | STALE | PENDING | LOCAL |
| LDG-Q005 | LDG-005 | Reconcile legacy focused claim through exact live consumer at RELEASED boundary | PENDING | PENDING | LOCAL |
| LDG-Q006 | LDG-006 | Reconcile legacy stale claim through exact live consumer at RELEASED boundary | STALE | PENDING | LOCAL |
| LDG-Q007 | LDG-007 | Reconcile legacy qualified claim through exact live consumer at RELEASED boundary | PENDING | PENDING | LOCAL |
| LDG-Q008 | LDG-008 | Reconcile legacy stale claim through exact live consumer at RELEASED boundary | STALE | PENDING | LOCAL |
| LDG-Q009 | LDG-009 | Reconcile legacy focused claim through exact live consumer at RELEASED boundary | PENDING | PENDING | LOCAL |
| LDG-Q010 | LDG-010 | Reconcile legacy qualified claim through exact live consumer at RELEASED boundary | PENDING | PENDING | LOCAL |
| LDG-Q011 | LDG-011 | Reconcile legacy qualified claim through exact live consumer at RELEASED boundary | PENDING | PENDING | LOCAL |
| LDG-Q012 | LDG-012 | Reconcile legacy qualified claim through exact live consumer at RELEASED boundary | PENDING | PENDING | LOCAL |
| LDG-Q013 | LDG-013 | Reconcile legacy qualified claim through exact live consumer at RELEASED boundary | PENDING | PENDING | LOCAL |
| LDG-Q014 | LDG-014 | Reconcile legacy focused claim through exact live consumer at RELEASED boundary | PENDING | PENDING | LOCAL |
| LDG-Q015 | LDG-015 | Reconcile legacy focused claim through exact live consumer at RELEASED boundary | PENDING | PENDING | LOCAL |
| LDG-Q016 | LDG-016 | Reconcile legacy focused claim through exact live consumer at RELEASED boundary | PENDING | PENDING | LOCAL |
| LDG-Q017 | LDG-017 | Reconcile legacy focused claim through exact live consumer at RELEASED boundary | PENDING | PENDING | LOCAL |
| LDG-Q018 | LDG-018 | Reconcile legacy focused claim through exact live consumer at RELEASED boundary | PENDING | PENDING | LOCAL |
| LDG-Q019 | LDG-019 | Reconcile legacy focused claim through exact live consumer at RELEASED boundary | PENDING | PENDING | LOCAL |
| LDG-Q020 | LDG-020 | Reconcile legacy focused claim through exact live consumer at RELEASED boundary | PENDING | PENDING | LOCAL |
| LDG-Q021 | LDG-021 | Reconcile legacy focused claim through exact live consumer at RELEASED boundary | PENDING | PENDING | LOCAL |
| LDG-Q022 | LDG-022 | Reconcile legacy mechanics claim through exact live consumer at RELEASED boundary | PENDING | PENDING | LOCAL |
| LDG-Q023 | LDG-023 | Reconcile legacy none claim through exact live consumer at RELEASED boundary | PENDING | PENDING | LOCAL |

## Decision register

| ID | Kind | Capability targets | Decision | Authority/evidence | State |
|---|---|---|---|---|---|
| LDG-D001 | REFERENCE | LDG-022, PUL-015 | Ledger owns section candidate materialization; Pull owns provider admission/invocation. | Canon reconciliation | RECORDED |
| LDG-D002 | EXCLUSION | LDG-001 | Guide/Spine current-product identity, durable memory, document truth & final relevance planning remain excluded. | Current Ledger architecture | RECORDED |
| LDG-D003 | REFERENCE | LDG-023 | Semantic structural-evidence producer remains exploratory because current Ledger architecture does not commit frozen proposal. | Frozen semantic compilation proposal | RECORDED |
