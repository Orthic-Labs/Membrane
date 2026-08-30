# Cortex atomic capability canon

Normalized from pre-standardization worktree canon inventory based on `d84322c3df182ff1d6ef7ca96fe94aea22273894`. Required delivery boundary: `RELEASED`.

Only committed capability rows count. Implementation, verification, qualification, delivery & evidence remain independent; closure is derived.

## Group register

| ID | Parent | Owner | Scope | Derived rollup |
|---|---|---|---|---|
| CTX-G01 | — | Cortex | COMMITTED | 34 committed capabilities; 1 exploratory capability; closure derived from child rows |

## Capability ledger

| ID | Parent | Owner | Scope | Observable behavior | Implementation | Verification | Qualification | Delivery | Action | Evidence |
|---|---|---|---|---|---|---|---|---|---|---|
| CTX-001 | CTX-G01 | Cortex | COMMITTED | Open one canonical local SQLite durable-knowledge authority with migrations/WAL integrity & no resident service identity. | UNKNOWN | PENDING | PENDING | LOCAL | RECONCILE_EVIDENCE | PENDING |
| CTX-002 | CTX-G01 | Cortex | COMMITTED | Pre-gate one durable candidate by schema, scope, producer, DLP, epistemic class & stable identity before semantic admission. | PARTIAL | PENDING | PENDING | LOCAL | REPAIR_WIRE | PENDING |
| CTX-003 | CTX-G01 | Cortex | COMMITTED | Batch governed durable writes atomically with per-item receipts & idempotency. | UNKNOWN | PENDING | PENDING | LOCAL | RECONCILE_EVIDENCE | PENDING |
| CTX-004 | CTX-G01 | Cortex | COMMITTED | Bind record to canonical meaning, scope/lineage, evidence, authority, sensitivity, temporal/lifecycle, supersession, & derivation. | PARTIAL | PENDING | PENDING | LOCAL | REPAIR_WIRE | PENDING |
| CTX-005 | CTX-G01 | Cortex | COMMITTED | Return exact semantic duplicate as typed successful no-op with existing identity. | UNKNOWN | PENDING | PENDING | LOCAL | RECONCILE_EVIDENCE | PENDING |
| CTX-006 | CTX-G01 | Cortex | COMMITTED | Detect scope-local near duplicates transactionally & update evidence metadata or no-op. | UNKNOWN | PENDING | PENDING | LOCAL | RECONCILE_EVIDENCE | PENDING |
| CTX-007 | CTX-G01 | Cortex | COMMITTED | Quarantine ambiguous conflicts outside active recall with typed receipt & governed restoration. | UNKNOWN | PENDING | PENDING | LOCAL | RECONCILE_EVIDENCE | PENDING |
| CTX-008 | CTX-G01 | Cortex | COMMITTED | Supersede without delete, retain both evidence histories, preserve simultaneous conflict, & surface deterministically. | UNKNOWN | PENDING | PENDING | LOCAL | RECONCILE_EVIDENCE | PENDING |
| CTX-009 | CTX-G01 | Cortex | COMMITTED | Answer point-in-time recall with observed, valid, recorded, & expiry semantics distinct. | UNKNOWN | PENDING | PENDING | LOCAL | RECONCILE_EVIDENCE | PENDING |
| CTX-010 | CTX-G01 | Cortex | COMMITTED | Apply deterministic versioned archive-first lifecycle; time triggers review but never rewrites authority. | PARTIAL | PENDING | PENDING | LOCAL | REPAIR_WIRE | PENDING |
| CTX-011 | CTX-G01 | Cortex | COMMITTED | Recall active/applicable durable knowledge through safe FTS5 lexical search under scope/time/lifecycle gates. | UNKNOWN | PENDING | PENDING | LOCAL | RECONCILE_EVIDENCE | PENDING |
| CTX-012 | CTX-G01 | Cortex | COMMITTED | Recall through local vector similarity with host-policy kernels & exact fallback, without remote correctness dependency. | UNKNOWN | PENDING | STALE | LOCAL | RECONCILE_EVIDENCE | PENDING |
| CTX-013 | CTX-G01 | Cortex | COMMITTED | Route/fuse lexical & vector memory evidence without confusing scores, authority, or Membrane final-planner ownership. | PARTIAL | PENDING | PENDING | LOCAL | REPAIR_WIRE | PENDING |
| CTX-014 | CTX-G01 | Cortex | COMMITTED | Return bounded preview/handle in recall, fetch full record by ID on demand, & record observed resolver use. | UNKNOWN | PENDING | PENDING | LOCAL | RECONCILE_EVIDENCE | PENDING |
| CTX-015 | CTX-G01 | Cortex | COMMITTED | Apply receipt-bound recall feedback to mutable usefulness signals without changing canonical content or authority. | PARTIAL | PENDING | PENDING | LOCAL | REPAIR_WIRE | PENDING |
| CTX-016 | CTX-G01 | Cortex | COMMITTED | Close unmatched deliveries as provisional unknown after bounded window, never infer ignored or failed. | UNKNOWN | PENDING | PENDING | LOCAL | RECONCILE_EVIDENCE | PENDING |
| CTX-017 | CTX-G01 | Cortex | COMMITTED | Maintain evidence relations `supports`, `contradicts`, `supersedes`, & `derived_from` without generic graph expansion. | PARTIAL | PENDING | PENDING | LOCAL | REPAIR_WIRE | PENDING |
| CTX-018 | CTX-G01 | Cortex | COMMITTED | Save/load/list/retire machine-local A0 checkpoints outside ordinary semantic recall. | UNKNOWN | PENDING | PENDING | LOCAL | RECONCILE_EVIDENCE | PENDING |
| CTX-019 | CTX-G01 | Cortex | COMMITTED | Promote checkpoint content only as normal governed knowledge proposal. | UNKNOWN | PENDING | PENDING | LOCAL | RECONCILE_EVIDENCE | PENDING |
| CTX-020 | CTX-G01 | Cortex | COMMITTED | Accept bounded `KnowledgeEmission` into durable pending/quarantine store with readback; never auto-admit model proposal. | UNKNOWN | PENDING | PENDING | LOCAL | RECONCILE_EVIDENCE | PENDING |
| CTX-021 | CTX-G01 | Cortex | COMMITTED | Review one pending proposal once as approved/rejected; admit approval & restore pending on admission failure. | UNKNOWN | PENDING | PENDING | LOCAL | RECONCILE_EVIDENCE | PENDING |
| CTX-022 | CTX-G01 | Cortex | COMMITTED | Run deterministic reversible Dream Stage 0 curation for duplicates, dates, low-value rows, restriction, & quarantine. | UNKNOWN | PENDING | PENDING | LOCAL | RECONCILE_EVIDENCE | PENDING |
| CTX-023 | CTX-G01 | Cortex | COMMITTED | Run semantic Dream Stage 1 as proposal-first contradiction/near-duplicate/supersession review with recoverable parents. | PARTIAL | PENDING | PENDING | LOCAL | REPAIR_WIRE | PENDING |
| CTX-024 | CTX-G01 | Cortex | COMMITTED | Extract bounded episodic memory proposals from event window only when authoritative foreground memory does not cover cursor range. | PARTIAL | PENDING | PENDING | LOCAL | REPAIR_WIRE | PENDING |
| CTX-025 | CTX-G01 | Cortex | COMMITTED | Hard-erase payload from every Cortex projection/link/tombstone while retaining only content-free erase event. | UNKNOWN | PENDING | PENDING | LOCAL | RECONCILE_EVIDENCE | PENDING |
| CTX-026 | CTX-G01 | Cortex | COMMITTED | Create digest-sealed backup & transactionally restore active/quarantined rows + links with tamper refusal & recall equivalence. | UNKNOWN | PENDING | PENDING | LOCAL | RECONCILE_EVIDENCE | PENDING |
| CTX-027 | CTX-G01 | Cortex | COMMITTED | Export/import canonical scoped Markdown/audit trees while DB remains authority. | UNKNOWN | PENDING | PENDING | LOCAL | RECONCILE_EVIDENCE | PENDING |
| CTX-028 | CTX-G01 | Cortex | COMMITTED | Export deterministic content-optional review queue for governed inspection. | UNKNOWN | PENDING | PENDING | LOCAL | RECONCILE_EVIDENCE | PENDING |
| CTX-029 | CTX-G01 | Cortex | COMMITTED | Rebuild vector/FTS projections from canonical content after index/embedder change. | UNKNOWN | PENDING | PENDING | LOCAL | RECONCILE_EVIDENCE | PENDING |
| CTX-030 | CTX-G01 | Cortex | COMMITTED | Explain/browse record metadata, scopes, lifecycle, provenance, retention reason, conflicts, & bounded relationship neighborhood read-only. | PARTIAL | PENDING | PENDING | LOCAL | REPAIR_WIRE | PENDING |
| CTX-031 | CTX-G01 | Cortex | COMMITTED | Register immutable causal-learning experiment, qualify only trusted controlled evidence, & expose promotion receipt. | UNKNOWN | PENDING | PENDING | LOCAL | RECONCILE_EVIDENCE | PENDING |
| CTX-032 | CTX-G01 | Cortex | COMMITTED | Persist/query append-only session/task/artifact events & content-free context telemetry by declared purpose, separate from authored truth. | UNKNOWN | PENDING | PENDING | LOCAL | RECONCILE_EVIDENCE | PENDING |
| CTX-033 | CTX-G01 | Cortex | EXPLORATORY | Admit/revalidate document-derived semantic knowledge only as Ledger-evidence-bound candidates through normal Cortex gates. | MISSING | PENDING | PENDING | LOCAL | HOLD | PENDING |
| CTX-034 | CTX-G01 | Cortex | COMMITTED | Ingest portable skill bodies into governed local index & return bounded resolver-backed content to Pull. | UNKNOWN | PENDING | PENDING | LOCAL | RECONCILE_EVIDENCE | PENDING |
| CTX-035 | CTX-G01 | Cortex | COMMITTED | Evaluate utility eligibility before admitting a durable candidate without replacing novelty, conflict or lifecycle gates. | UNKNOWN | PENDING | PENDING | LOCAL | RECONCILE_EVIDENCE | PENDING |

## Implementation register

| ID | Capability targets | Mechanism | Source/donor | Reuse mode | State | Production consumer |
|---|---|---|---|---|---|---|
| CTX-I001 | CTX-001 | `engine/crates/cortex-store/src/memdb.rs`; `engine/crates/cortex-store/src/db.rs`; store tests | Legacy pre-normalization row cortex.md:7 (worktree base d84322c3df182ff1d6ef7ca96fe94aea22273894); dependencies: MEM-018; canon: MCA §0.3, §8; note: Legacy models increase convergence risk. | ADAPT | UNKNOWN | Tray daemon/CLI |
| CTX-I002 | CTX-002 | runtime `store.rs`; admission tests | Legacy pre-normalization row cortex.md:8 (worktree base d84322c3df182ff1d6ef7ca96fe94aea22273894); dependencies: MEM-004, CTX-001; canon: MCA §8.1; ADC §7.1; note: Full ordered gate not frozen end-to-end. | ADAPT | PARTIAL | Native writer/Adapt |
| CTX-I003 | CTX-003 | `store.rs::try_put_batch`; batch tests | Legacy pre-normalization row cortex.md:9 (worktree base d84322c3df182ff1d6ef7ca96fe94aea22273894); dependencies: CTX-002; canon: MCA §8.1; note: — | ADAPT | UNKNOWN | HTTP/native batch caller |
| CTX-I004 | CTX-004 | memdb schema; cortex-core types/tests | Legacy pre-normalization row cortex.md:10 (worktree base d84322c3df182ff1d6ef7ca96fe94aea22273894); dependencies: CTX-002; canon: MCA §8.2; note: Canonical shape not fully converged. | ADAPT | PARTIAL | Retrieval/inspection |
| CTX-I005 | CTX-005 | admission disposition; lifecycle tests | Legacy pre-normalization row cortex.md:11 (worktree base d84322c3df182ff1d6ef7ca96fe94aea22273894); dependencies: CTX-002; canon: MCA §8.3; MPI §16.3; note: — | ADAPT | UNKNOWN | Durable writer |
| CTX-I006 | CTX-006 | memdb/store lifecycle tests | Legacy pre-normalization row cortex.md:12 (worktree base d84322c3df182ff1d6ef7ca96fe94aea22273894); dependencies: CTX-005; canon: MCA §8.4; MPI §16.3; note: Cross-process contention fixture absent. | ADAPT | UNKNOWN | Durable writer/reviewer |
| CTX-I007 | CTX-007 | store review/quarantine APIs; lifecycle tests | Legacy pre-normalization row cortex.md:13 (worktree base d84322c3df182ff1d6ef7ca96fe94aea22273894); dependencies: CTX-002, CTX-006; canon: MCA §8.4; note: Broad review UI absent. | ADAPT | UNKNOWN | Reviewer/operator |
| CTX-I008 | CTX-008 | `engine/crates/cortex-store/src/temporal.rs`; temporal tests | Legacy pre-normalization row cortex.md:14 (worktree base d84322c3df182ff1d6ef7ca96fe94aea22273894); dependencies: CTX-004; canon: MCA §8.4–8.6; MPI §16.2; note: — | ADAPT | UNKNOWN | Recall/as-of caller |
| CTX-I009 | CTX-009 | temporal store; `membrane_temporal_fact`; tests | Legacy pre-normalization row cortex.md:15 (worktree base d84322c3df182ff1d6ef7ca96fe94aea22273894); dependencies: CTX-008; canon: MCA §8.5; note: Current installed scenario absent. | ADAPT | UNKNOWN | MCP/CLI temporal caller |
| CTX-I010 | CTX-010 | registry/effectiveness/review tests | Legacy pre-normalization row cortex.md:16 (worktree base d84322c3df182ff1d6ef7ca96fe94aea22273894); dependencies: CTX-008; canon: MCA §8.6; XSG §7.2; note: Ledger/Blueprint/version/outcome triggers incomplete. | ADAPT | PARTIAL | Recall/curation |
| CTX-I011 | CTX-011 | `engine/crates/cortex-store/src/fts5.rs`; hybrid tests | Legacy pre-normalization row cortex.md:17 (worktree base d84322c3df182ff1d6ef7ca96fe94aea22273894); dependencies: CTX-001, CTX-010; canon: MCA §3, §8; note: Installed relevance proof absent. | ADAPT | UNKNOWN | Pull Cortex provider |
| CTX-I012 | CTX-012 | `engine/crates/cortex-core/src/vector_index.rs`; vector tests; historical v0.1.12 evidence | Legacy pre-normalization row cortex.md:18 (worktree base d84322c3df182ff1d6ef7ca96fe94aea22273894); dependencies: CTX-001, CTX-010; canon: MCA §3; note: Ancestor installed qualification does not cover HEAD. | ADAPT | UNKNOWN | Pull Cortex provider |
| CTX-I013 | CTX-013 | cortex-core routing/retriever tests | Legacy pre-normalization row cortex.md:19 (worktree base d84322c3df182ff1d6ef7ca96fe94aea22273894); dependencies: CTX-011–CTX-012; canon: MCA §3–4; note: Legacy Cortex planner must remain provider-local. | ADAPT | PARTIAL | Pull Cortex provider |
| CTX-I014 | CTX-014 | retriever; `get_full_observed`; public API tests | Legacy pre-normalization row cortex.md:20 (worktree base d84322c3df182ff1d6ef7ca96fe94aea22273894); dependencies: CTX-013; canon: MCA §7.1, §8; note: Observed use is not verified usefulness. | ADAPT | UNKNOWN | Agent/Pull |
| CTX-I015 | CTX-015 | `effectiveness.rs`; telemetry; feedback tests | Legacy pre-normalization row cortex.md:21 (worktree base d84322c3df182ff1d6ef7ca96fe94aea22273894); dependencies: MEM-024, CTX-014; canon: MCA §7.1, §8.2; note: Verified producers incomplete. | ADAPT | PARTIAL | Host/verifier/retriever |
| CTX-I016 | CTX-016 | `close_unresolved_deliveries`; CLI tests | Legacy pre-normalization row cortex.md:22 (worktree base d84322c3df182ff1d6ef7ca96fe94aea22273894); dependencies: CTX-015; canon: MCA §7; note: — | ADAPT | UNKNOWN | Evaluation operator |
| CTX-I017 | CTX-017 | cortex-core graph; memdb links; tests | Legacy pre-normalization row cortex.md:23 (worktree base d84322c3df182ff1d6ef7ca96fe94aea22273894); dependencies: CTX-004; canon: MCA §8.2; note: Relation neighborhood must stay bounded. | ADAPT | PARTIAL | Inspection/lifecycle |
| CTX-I018 | CTX-018 | runtime `checkpoint.rs`; tests | Legacy pre-normalization row cortex.md:24 (worktree base d84322c3df182ff1d6ef7ca96fe94aea22273894); dependencies: MEM-004, LDG-005; canon: MCA session lane; note: Distinct from Membrane working context & Ledger session projection. | ADAPT | UNKNOWN | MCP/CLI session host |
| CTX-I019 | CTX-019 | checkpoint promote CLI/tests | Legacy pre-normalization row cortex.md:25 (worktree base d84322c3df182ff1d6ef7ca96fe94aea22273894); dependencies: CTX-002, CTX-018; canon: MCA §8.1; note: Never direct truth. | ADAPT | UNKNOWN | Operator/reviewer |
| CTX-I020 | CTX-020 | `membrane_knowledge_propose`; proposal-store tests | Legacy pre-normalization row cortex.md:26 (worktree base d84322c3df182ff1d6ef7ca96fe94aea22273894); dependencies: MEM-004, CTX-002; canon: MCA §8.1, §14.1; note: Persistence failure is explicit error. | ADAPT | UNKNOWN | MCP caller/reviewer |
| CTX-I021 | CTX-021 | review/store tests | Legacy pre-normalization row cortex.md:27 (worktree base d84322c3df182ff1d6ef7ca96fe94aea22273894); dependencies: CTX-002, CTX-020; canon: ADC §3.5; MPI §16.1; note: Automatic approval explicitly absent. | ADAPT | UNKNOWN | Native reviewer/Adapt |
| CTX-I022 | CTX-022 | `engine/crates/cortex-core/src/dream.rs`; curation tests | Legacy pre-normalization row cortex.md:28 (worktree base d84322c3df182ff1d6ef7ca96fe94aea22273894); dependencies: CTX-007, CTX-010; canon: MCA §8.7; MPI §5; note: Effect qualification absent. | ADAPT | UNKNOWN | CLI/background operator |
| CTX-I023 | CTX-023 | review contracts; background semantic tests | Legacy pre-normalization row cortex.md:29 (worktree base d84322c3df182ff1d6ef7ca96fe94aea22273894); dependencies: CTX-021–CTX-022, MEM-053; canon: MCA §8.7; MPI §5; note: Real semantic provider/foreground signal/sink not proven. | ADAPT | PARTIAL | Background review daemon |
| CTX-I024 | CTX-024 | cortex review + daemon input tests | Legacy pre-normalization row cortex.md:30 (worktree base d84322c3df182ff1d6ef7ca96fe94aea22273894); dependencies: CTX-021, CTX-032, MEM-053; canon: MCA §8.8; MPI §5.1; note: Cortex-specific proposal sink unproven. | ADAPT | PARTIAL | Background review daemon |
| CTX-I025 | CTX-025 | `store.rs::hard_erase`; lifecycle tests | Legacy pre-normalization row cortex.md:31 (worktree base d84322c3df182ff1d6ef7ca96fe94aea22273894); dependencies: CTX-001, CTX-007; canon: MCA §14.4; MPI §16.4; note: No operational surface; link-specific proof incomplete. | ADAPT | UNKNOWN | Future authorized operator |
| CTX-I026 | CTX-026 | backup/restore APIs; lifecycle tests | Legacy pre-normalization row cortex.md:32 (worktree base d84322c3df182ff1d6ef7ca96fe94aea22273894); dependencies: CTX-001, CTX-007; canon: MCA §14.4–14.5, §16; note: No authorized external surface. | ADAPT | UNKNOWN | Disaster-recovery operator |
| CTX-I027 | CTX-027 | CLI export/import; db-first tests | Legacy pre-normalization row cortex.md:33 (worktree base d84322c3df182ff1d6ef7ca96fe94aea22273894); dependencies: CTX-001; canon: MCA §8.2, §17; note: Export erasure coverage not frozen. | ADAPT | UNKNOWN | Operator/migration |
| CTX-I028 | CTX-028 | vault export API/CLI tests | Legacy pre-normalization row cortex.md:34 (worktree base d84322c3df182ff1d6ef7ca96fe94aea22273894); dependencies: CTX-001; canon: MCA §17; note: Review surface only. | ADAPT | UNKNOWN | Operator/reviewer |
| CTX-I029 | CTX-029 | CLI `reindex`; rebuild tests | Legacy pre-normalization row cortex.md:35 (worktree base d84322c3df182ff1d6ef7ca96fe94aea22273894); dependencies: CTX-001, CTX-011–CTX-012; canon: MCA §14.5; note: Erased-data non-reappearance proof incomplete. | ADAPT | UNKNOWN | Maintenance operator |
| CTX-I030 | CTX-030 | CLI explain/list/graph; Hub sentinel tests | Legacy pre-normalization row cortex.md:36 (worktree base d84322c3df182ff1d6ef7ca96fe94aea22273894); dependencies: CTX-004, CTX-017; canon: MCA §17; note: No one complete canonical inspection surface. | ADAPT | PARTIAL | Operator/Hub |
| CTX-I031 | CTX-031 | calibration/eval gate; causal-promotion tests | Legacy pre-normalization row cortex.md:37 (worktree base d84322c3df182ff1d6ef7ca96fe94aea22273894); dependencies: CTX-015–CTX-016; canon: MCA §7, §18; note: Missing H7/H9/H10 blocks broad claims. | ADAPT | UNKNOWN | Adapt/evaluation operator |
| CTX-I032 | CTX-032 | `absorbed_records.rs`; `context_telemetry.rs`; tests | Legacy pre-normalization row cortex.md:38 (worktree base d84322c3df182ff1d6ef7ca96fe94aea22273894); dependencies: CTX-001; canon: MCA §7–8; note: Raw telemetry is not durable knowledge. | ADAPT | UNKNOWN | Daemon/Adapt/CodeRight |
| CTX-I033 | CTX-033 | No production candidate type/compiler | Legacy pre-normalization row cortex.md:39 (worktree base d84322c3df182ff1d6ef7ca96fe94aea22273894); dependencies: CTX-002, LDG-023; canon: XSG §7.4; frozen `docs/pending/semantic-blueprint-review-pack-v2/02-CORTEX-DOCUMENT-SEMANTIC-KNOWLEDGE-AMENDMENT.md@d84322c3df182ff1d6ef7ca96fe94aea22273894`; note: Auto-admission remains prohibited. | ADAPT | MISSING | Future Pull semantic lane |
| CTX-I034 | CTX-034 | `engine/crates/membrane-runtime/src/store.rs`; Cortex skill-read tests | Legacy pre-normalization row cortex.md:40 (worktree base d84322c3df182ff1d6ef7ca96fe94aea22273894); dependencies: CTX-001, CTX-029; canon: MCA evidence class ownership; note: Disk-first with Cortex fallback; not generic memory authority. | ADAPT | UNKNOWN | Pull skills provider/CLI |
| CTX-I035 | CTX-035 | Cortex admission utility gate | Split from CTX-002 after atomicity reconciliation; utility closes independently from pre-gate, novelty, conflict & lifecycle | ADAPT | UNKNOWN | Native writer/Adapt |

## Qualification ledger

| ID | Capability targets | Acceptance boundary | State | Evidence | Material revision |
|---|---|---|---|---|---|
| CTX-Q001 | CTX-001 | Reconcile legacy focused claim through exact live consumer at RELEASED boundary | PENDING | PENDING | LOCAL |
| CTX-Q002 | CTX-002 | Reconcile legacy focused claim through exact live consumer at RELEASED boundary | PENDING | PENDING | LOCAL |
| CTX-Q003 | CTX-003 | Reconcile legacy focused claim through exact live consumer at RELEASED boundary | PENDING | PENDING | LOCAL |
| CTX-Q004 | CTX-004 | Reconcile legacy focused claim through exact live consumer at RELEASED boundary | PENDING | PENDING | LOCAL |
| CTX-Q005 | CTX-005 | Reconcile legacy focused claim through exact live consumer at RELEASED boundary | PENDING | PENDING | LOCAL |
| CTX-Q006 | CTX-006 | Reconcile legacy focused claim through exact live consumer at RELEASED boundary | PENDING | PENDING | LOCAL |
| CTX-Q007 | CTX-007 | Reconcile legacy focused claim through exact live consumer at RELEASED boundary | PENDING | PENDING | LOCAL |
| CTX-Q008 | CTX-008 | Reconcile legacy focused claim through exact live consumer at RELEASED boundary | PENDING | PENDING | LOCAL |
| CTX-Q009 | CTX-009 | Reconcile legacy focused claim through exact live consumer at RELEASED boundary | PENDING | PENDING | LOCAL |
| CTX-Q010 | CTX-010 | Reconcile legacy focused claim through exact live consumer at RELEASED boundary | PENDING | PENDING | LOCAL |
| CTX-Q011 | CTX-011 | Reconcile legacy focused claim through exact live consumer at RELEASED boundary | PENDING | PENDING | LOCAL |
| CTX-Q012 | CTX-012 | Reconcile legacy stale claim through exact live consumer at RELEASED boundary | STALE | PENDING | LOCAL |
| CTX-Q013 | CTX-013 | Reconcile legacy focused claim through exact live consumer at RELEASED boundary | PENDING | PENDING | LOCAL |
| CTX-Q014 | CTX-014 | Reconcile legacy focused claim through exact live consumer at RELEASED boundary | PENDING | PENDING | LOCAL |
| CTX-Q015 | CTX-015 | Reconcile legacy focused claim through exact live consumer at RELEASED boundary | PENDING | PENDING | LOCAL |
| CTX-Q016 | CTX-016 | Reconcile legacy focused claim through exact live consumer at RELEASED boundary | PENDING | PENDING | LOCAL |
| CTX-Q017 | CTX-017 | Reconcile legacy focused claim through exact live consumer at RELEASED boundary | PENDING | PENDING | LOCAL |
| CTX-Q018 | CTX-018 | Reconcile legacy focused claim through exact live consumer at RELEASED boundary | PENDING | PENDING | LOCAL |
| CTX-Q019 | CTX-019 | Reconcile legacy focused claim through exact live consumer at RELEASED boundary | PENDING | PENDING | LOCAL |
| CTX-Q020 | CTX-020 | Reconcile legacy focused claim through exact live consumer at RELEASED boundary | PENDING | PENDING | LOCAL |
| CTX-Q021 | CTX-021 | Reconcile legacy focused claim through exact live consumer at RELEASED boundary | PENDING | PENDING | LOCAL |
| CTX-Q022 | CTX-022 | Reconcile legacy focused claim through exact live consumer at RELEASED boundary | PENDING | PENDING | LOCAL |
| CTX-Q023 | CTX-023 | Reconcile legacy focused claim through exact live consumer at RELEASED boundary | PENDING | PENDING | LOCAL |
| CTX-Q024 | CTX-024 | Reconcile legacy focused claim through exact live consumer at RELEASED boundary | PENDING | PENDING | LOCAL |
| CTX-Q025 | CTX-025 | Reconcile legacy focused claim through exact live consumer at RELEASED boundary | PENDING | PENDING | LOCAL |
| CTX-Q026 | CTX-026 | Reconcile legacy focused claim through exact live consumer at RELEASED boundary | PENDING | PENDING | LOCAL |
| CTX-Q027 | CTX-027 | Reconcile legacy focused claim through exact live consumer at RELEASED boundary | PENDING | PENDING | LOCAL |
| CTX-Q028 | CTX-028 | Reconcile legacy focused claim through exact live consumer at RELEASED boundary | PENDING | PENDING | LOCAL |
| CTX-Q029 | CTX-029 | Reconcile legacy focused claim through exact live consumer at RELEASED boundary | PENDING | PENDING | LOCAL |
| CTX-Q030 | CTX-030 | Reconcile legacy focused claim through exact live consumer at RELEASED boundary | PENDING | PENDING | LOCAL |
| CTX-Q031 | CTX-031 | Reconcile legacy focused claim through exact live consumer at RELEASED boundary | PENDING | PENDING | LOCAL |
| CTX-Q032 | CTX-032 | Reconcile legacy focused claim through exact live consumer at RELEASED boundary | PENDING | PENDING | LOCAL |
| CTX-Q033 | CTX-033 | Reconcile legacy none claim through exact live consumer at RELEASED boundary | PENDING | PENDING | LOCAL |
| CTX-Q034 | CTX-034 | Reconcile legacy focused claim through exact live consumer at RELEASED boundary | PENDING | PENDING | LOCAL |
| CTX-Q035 | CTX-035 | Prove utility eligibility through durable admission production path at RELEASED boundary | PENDING | PENDING | LOCAL |

## Decision register

| ID | Kind | Capability targets | Decision | Authority/evidence | State |
|---|---|---|---|---|---|
| CTX-D001 | REFERENCE | CTX-015, MEM-024 | Cortex owns recall-usefulness application; Membrane owns feedback-strength classification. | Canon reconciliation | RECORDED |
| CTX-D002 | EXCLUSION | CTX-001 | Cortex is durable knowledge, not working context, Ledger projection, repository truth, or an independently resident service. | Current Cortex architecture | RECORDED |
| CTX-D003 | REFERENCE | CTX-002, CTX-005, CTX-006, CTX-007, CTX-008, CTX-010, CTX-035 | Pre-gate, exact/near duplicate, conflict/quarantine, lifecycle & utility gates close independently. | Atomicity reconciliation | RECORDED |
| CTX-D004 | REFERENCE | CTX-033 | Document-derived semantic knowledge remains exploratory because current Cortex architecture does not commit frozen proposal. | Frozen semantic compilation proposal | RECORDED |
