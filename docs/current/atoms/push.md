# Push atomic capability canon

Normalized from pre-standardization worktree canon inventory based on `d84322c3df182ff1d6ef7ca96fe94aea22273894`. Required delivery boundary: `RELEASED`.

Only committed capability rows count. Implementation, verification, qualification, delivery & evidence remain independent; closure is derived.

## Group register

| ID | Parent | Owner | Scope | Derived rollup |
|---|---|---|---|---|
| PSH-G01 | — | Push | COMMITTED | 17 committed capabilities; closure derived from child rows |

## Capability ledger

| ID | Parent | Owner | Scope | Observable behavior | Implementation | Verification | Qualification | Delivery | Action | Evidence |
|---|---|---|---|---|---|---|---|---|---|---|
| PSH-001 | PSH-G01 | Push | COMMITTED | Cap command output to deterministic head/tail, spill omitted bytes, & emit recoverable anchor. | UNKNOWN | PENDING | PENDING | LOCAL | RECONCILE_EVIDENCE | PENDING |
| PSH-002 | PSH-G01 | Push | COMMITTED | Restore exact spilled output by opaque anchor from confined artifact storage. | UNKNOWN | PENDING | PENDING | LOCAL | RECONCILE_EVIDENCE | PENDING |
| PSH-003 | PSH-G01 | Push | COMMITTED | Skeletonize supported source/structured files under budget while preserving structure, identifiers, & protected spans. | UNKNOWN | PENDING | PENDING | LOCAL | RECONCILE_EVIDENCE | PENDING |
| PSH-004 | PSH-G01 | Push | COMMITTED | Apply extractive bounded text compression with deterministic fallback/passthrough. | UNKNOWN | PENDING | PENDING | LOCAL | RECONCILE_EVIDENCE | PENDING |
| PSH-005 | PSH-G01 | Push | COMMITTED | Externalize raw input content-addressably before lossy reduction & verify recovery marker. | UNKNOWN | PENDING | PENDING | LOCAL | RECONCILE_EVIDENCE | PENDING |
| PSH-006 | PSH-G01 | Push | COMMITTED | Batch-prepare files into reversible artifact-backed representations under one shared budget without source mutation. | UNKNOWN | PENDING | PENDING | LOCAL | RECONCILE_EVIDENCE | PENDING |
| PSH-007 | PSH-G01 | Push | COMMITTED | Select query-aware reduction only with admitted query, authority, & valid freshness. | UNKNOWN | PENDING | PENDING | LOCAL | RECONCILE_EVIDENCE | PENDING |
| PSH-008 | PSH-G01 | Push | COMMITTED | Prepare tool/MCP-result egress through same reversible transform contract before agent rendering. | PARTIAL | PENDING | PENDING | LOCAL | REPAIR_WIRE | PENDING |
| PSH-009 | PSH-G01 | Push | COMMITTED | Apply same reversible contract to large governed source/document reads. | PARTIAL | PENDING | PENDING | LOCAL | REPAIR_WIRE | PENDING |
| PSH-010 | PSH-G01 | Push | COMMITTED | Let providers cap/externalize raw payloads while Membrane planner retains eligibility & representation authority. | PARTIAL | PENDING | PENDING | LOCAL | REPAIR_WIRE | PENDING |
| PSH-011 | PSH-G01 | Push | COMMITTED | Validate full/reduced/floor packet plan & choose largest complete representation fitting request-time H8 ceiling. | UNKNOWN | PENDING | PENDING | LOCAL | RECONCILE_EVIDENCE | PENDING |
| PSH-012 | PSH-G01 | Push | COMMITTED | Refuse packet selection when host capacity/basis is missing, stale, or mismatched; never guess or silently drop items. | UNKNOWN | PENDING | PENDING | LOCAL | RECONCILE_EVIDENCE | PENDING |
| PSH-013 | PSH-G01 | Push | COMMITTED | Apply explicit truncation last & fall back toward less reduction/raw on uncertainty or transform failure. | UNKNOWN | PENDING | PENDING | LOCAL | RECONCILE_EVIDENCE | PENDING |
| PSH-014 | PSH-G01 | Push | COMMITTED | Preserve or exactly restore IDs, errors, tests, values, policies, task entities, tool/result pairs, decisions/rationales, & diff spans. | PARTIAL | PENDING | PENDING | LOCAL | REPAIR_WIRE | PENDING |
| PSH-015 | PSH-G01 | Push | COMMITTED | Preserve planner evidence order unless explicit versioned ordering policy selects otherwise. | PARTIAL | PENDING | PENDING | LOCAL | REPAIR_WIRE | PENDING |
| PSH-016 | PSH-G01 | Push | COMMITTED | Emit original/materialized/delivered/provider-billed balance with fidelity/recoverability assertion. | PARTIAL | PENDING | PENDING | LOCAL | REPAIR_WIRE | PENDING |
| PSH-017 | PSH-G01 | Push | COMMITTED | Report opportunities, executions, passthroughs, externalization, avoided bytes/tokens, restores, failures, & task non-regression without inventing missing outcomes. | PARTIAL | PENDING | PENDING | LOCAL | REPAIR_WIRE | PENDING |

## Implementation register

| ID | Capability targets | Mechanism | Source/donor | Reuse mode | State | Production consumer |
|---|---|---|---|---|---|---|
| PSH-I001 | PSH-001 | `engine/crates/membrane-runtime/src/push/runc.rs`; protected-evidence tests | Legacy pre-normalization row push.md:7 (worktree base d84322c3df182ff1d6ef7ca96fe94aea22273894); dependencies: —; canon: MCA §9, §23.5; note: Host adoption/effect telemetry incomplete. | ADAPT | UNKNOWN | CLI/host tool execution |
| PSH-I002 | PSH-002 | `engine/crates/membrane-runtime/src/push/runc.rs`; restore/adversarial tests | Legacy pre-normalization row push.md:8 (worktree base d84322c3df182ff1d6ef7ca96fe94aea22273894); dependencies: PSH-001; canon: MCA §9.2–9.4; note: Restore outcome metrics absent. | ADAPT | UNKNOWN | CLI/host resolver |
| PSH-I003 | PSH-003 | `engine/crates/membrane-runtime/src/push/skel.rs`; `push_qualification.rs` | Legacy pre-normalization row push.md:9 (worktree base d84322c3df182ff1d6ef7ca96fe94aea22273894); dependencies: —; canon: MCA §9.2–9.3; note: Task-correctness qualification absent. | ADAPT | UNKNOWN | CLI/planner |
| PSH-I004 | PSH-004 | `engine/crates/membrane-runtime/src/push/compress.rs`; compression contract tests | Legacy pre-normalization row push.md:10 (worktree base d84322c3df182ff1d6ef7ca96fe94aea22273894); dependencies: PSH-003; canon: MCA §9.1–9.4; note: Model/provider artifact coverage varies. | ADAPT | UNKNOWN | CLI/planner |
| PSH-I005 | PSH-005 | `engine/crates/membrane-runtime/src/push/compress.rs`; protected-evidence tests | Legacy pre-normalization row push.md:11 (worktree base d84322c3df182ff1d6ef7ca96fe94aea22273894); dependencies: —; canon: MCA §9.2; note: Current installed proof absent. | ADAPT | UNKNOWN | Reducer/resolver |
| PSH-I006 | PSH-006 | `engine/crates/membrane-runtime/src/push/prep.rs`; `push-metrics.json` | Legacy pre-normalization row push.md:12 (worktree base d84322c3df182ff1d6ef7ca96fe94aea22273894); dependencies: PSH-003–PSH-005; canon: MCA §9.2; note: Mechanics only. | ADAPT | UNKNOWN | CLI/planner |
| PSH-I007 | PSH-007 | `engine/crates/membrane-runtime/src/push/prep.rs`; `push-metrics.json` | Legacy pre-normalization row push.md:13 (worktree base d84322c3df182ff1d6ef7ca96fe94aea22273894); dependencies: PSH-006, PUL-017; canon: XSG §6.2; MPI §10; note: Control remains default; outcome/latency proof open. | ADAPT | UNKNOWN | Planner experiment |
| PSH-I008 | PSH-008 | `mcp/host/context-adapter.cjs`; host capability tests | Legacy pre-normalization row push.md:14 (worktree base d84322c3df182ff1d6ef7ca96fe94aea22273894); dependencies: PSH-005–PSH-007; canon: MCA §9.1A–B; note: Generic MCP lacks rewrite hook; delegated Codex egress unavailable. | ADAPT | PARTIAL | Supported host adapters |
| PSH-I009 | PSH-009 | source-read + Push integration tests | Legacy pre-normalization row push.md:15 (worktree base d84322c3df182ff1d6ef7ca96fe94aea22273894); dependencies: PSH-003–PSH-007, LDG-005; canon: MCA §9.1C; note: One automatic path across all reads unproven. | ADAPT | PARTIAL | Ledger/source-read consumers |
| PSH-I010 | PSH-010 | provider SDK; Push preparation tests | Legacy pre-normalization row push.md:16 (worktree base d84322c3df182ff1d6ef7ca96fe94aea22273894); dependencies: PUL-027; canon: MCA §9.1D; note: All provider routes not acceptance-proven. | ADAPT | PARTIAL | Pull provider adapters |
| PSH-I011 | PSH-011 | `engine/crates/membrane-runtime/src/push/selection.rs`; H8 tests | Legacy pre-normalization row push.md:17 (worktree base d84322c3df182ff1d6ef7ca96fe94aea22273894); dependencies: PUL-027, PSH-003–PSH-007; canon: MCA §6.12, §9; MPI §11; note: No measured task proof. | ADAPT | UNKNOWN | Pull planner/host |
| PSH-I012 | PSH-012 | `engine/crates/membrane-runtime/src/push/selection.rs`; tests | Legacy pre-normalization row push.md:18 (worktree base d84322c3df182ff1d6ef7ca96fe94aea22273894); dependencies: PSH-011; canon: MCA §9.4; MPI §11; note: H8 producer breadth incomplete. | ADAPT | UNKNOWN | First-party host |
| PSH-I013 | PSH-013 | `engine/crates/membrane-runtime/src/push/truncate.rs`; Push qualification | Legacy pre-normalization row push.md:19 (worktree base d84322c3df182ff1d6ef7ca96fe94aea22273894); dependencies: PSH-003–PSH-012; canon: MCA §9.2–9.3; note: Current artifact unqualified. | ADAPT | UNKNOWN | Final renderer |
| PSH-I014 | PSH-014 | `tests/compression/protected-evidence.test.mjs`; `push-metrics.json` | Legacy pre-normalization row push.md:20 (worktree base d84322c3df182ff1d6ef7ca96fe94aea22273894); dependencies: PSH-005, PSH-013; canon: MCA §9.3; note: Fixture covers subset, not exhaustive classes. | ADAPT | PARTIAL | Every reduced packet |
| PSH-I015 | PSH-015 | `engine/crates/membrane-runtime/src/push/selection.rs`; ordering tests | Legacy pre-normalization row push.md:21 (worktree base d84322c3df182ff1d6ef7ca96fe94aea22273894); dependencies: PUL-026, PSH-011; canon: MCA §12; XSG §6.3; note: Alternative layout not qualified. | ADAPT | PARTIAL | Final packet renderer |
| PSH-I016 | PSH-016 | core reconciliation + Push telemetry tests | Legacy pre-normalization row push.md:22 (worktree base d84322c3df182ff1d6ef7ca96fe94aea22273894); dependencies: PSH-001–PSH-015; canon: MCA §9.4; note: Provider-billed cross-host basis incomplete. | ADAPT | PARTIAL | Packet receipts/evaluation |
| PSH-I017 | PSH-017 | `engine/crates/membrane-runtime/src/push/telemetry.rs`; `push-metrics.json` | Legacy pre-normalization row push.md:23 (worktree base d84322c3df182ff1d6ef7ca96fe94aea22273894); dependencies: PSH-016, MEM-024; canon: MCA §9.5; XSG §6.4; note: H6/H7 joins, restores, corrections, latency & task outcomes absent. | ADAPT | PARTIAL | Hub/Adapt/evaluation |

## Qualification ledger

| ID | Capability targets | Acceptance boundary | State | Evidence | Material revision |
|---|---|---|---|---|---|
| PSH-Q001 | PSH-001 | Reconcile legacy focused claim through exact live consumer at RELEASED boundary | PENDING | PENDING | LOCAL |
| PSH-Q002 | PSH-002 | Reconcile legacy focused claim through exact live consumer at RELEASED boundary | PENDING | PENDING | LOCAL |
| PSH-Q003 | PSH-003 | Reconcile legacy mechanics claim through exact live consumer at RELEASED boundary | PENDING | PENDING | LOCAL |
| PSH-Q004 | PSH-004 | Reconcile legacy mechanics claim through exact live consumer at RELEASED boundary | PENDING | PENDING | LOCAL |
| PSH-Q005 | PSH-005 | Reconcile legacy focused claim through exact live consumer at RELEASED boundary | PENDING | PENDING | LOCAL |
| PSH-Q006 | PSH-006 | Reconcile legacy mechanics claim through exact live consumer at RELEASED boundary | PENDING | PENDING | LOCAL |
| PSH-Q007 | PSH-007 | Reconcile legacy mechanics claim through exact live consumer at RELEASED boundary | PENDING | PENDING | LOCAL |
| PSH-Q008 | PSH-008 | Reconcile legacy focused claim through exact live consumer at RELEASED boundary | PENDING | PENDING | LOCAL |
| PSH-Q009 | PSH-009 | Reconcile legacy focused claim through exact live consumer at RELEASED boundary | PENDING | PENDING | LOCAL |
| PSH-Q010 | PSH-010 | Reconcile legacy focused claim through exact live consumer at RELEASED boundary | PENDING | PENDING | LOCAL |
| PSH-Q011 | PSH-011 | Reconcile legacy focused claim through exact live consumer at RELEASED boundary | PENDING | PENDING | LOCAL |
| PSH-Q012 | PSH-012 | Reconcile legacy focused claim through exact live consumer at RELEASED boundary | PENDING | PENDING | LOCAL |
| PSH-Q013 | PSH-013 | Reconcile legacy mechanics claim through exact live consumer at RELEASED boundary | PENDING | PENDING | LOCAL |
| PSH-Q014 | PSH-014 | Reconcile legacy mechanics claim through exact live consumer at RELEASED boundary | PENDING | PENDING | LOCAL |
| PSH-Q015 | PSH-015 | Reconcile legacy focused claim through exact live consumer at RELEASED boundary | PENDING | PENDING | LOCAL |
| PSH-Q016 | PSH-016 | Reconcile legacy focused claim through exact live consumer at RELEASED boundary | PENDING | PENDING | LOCAL |
| PSH-Q017 | PSH-017 | Reconcile legacy mechanics claim through exact live consumer at RELEASED boundary | PENDING | PENDING | LOCAL |

## Decision register

| ID | Kind | Capability targets | Decision | Authority/evidence | State |
|---|---|---|---|---|---|
| PSH-D001 | REFERENCE | PSH-015 | Push preserves planner-selected order; Pull/Membrane selects any versioned ordering policy. | Canon reconciliation | RECORDED |
| PSH-D002 | EXCLUSION | PSH-003 | Push never becomes a second planner or eligibility authority. | Current Push architecture | RECORDED |
