# Evidence-path & corpus-health canon reconciliation

Date: 2026-09-02  
Mode: Foundation `NORMALIZE` + `RECONCILE`  
Current-product revision inspected: `9481f2fc9eabec0879947ae59a0fc2b54ed3e1e9`

## Scope

Requested: add one exploratory evidence-path capability, repair existing ownership/acceptance boundaries, reconcile MCP discovery evidence, & bind proposed evaluations to qualification rows.

Evaluated: 16 capability rows across Cortex, Membrane, & Pull: `CTX-004`, `CTX-017`, `CTX-038`, `CTX-039`, `MEM-013`, `MEM-029`, `MEM-061`–`MEM-066`, `PUL-017`, `PUL-037`, `PUL-039`, & `PUL-040`.

Unresolved: every changed qualification remains `PENDING`; `CTX-039`, `PUL-037`, `PUL-039`, & `PUL-040` remain `MISSING`; `CTX-004`, `CTX-017`, `MEM-029`, `MEM-061`, `MEM-062`, & `PUL-017` remain `PARTIAL`.

Excluded: generic entity ontology, alias/master-data registry, Neo4j/Kuzu adoption, new Corpus Audit atom, new scheduler subsystem, execution-harness sandbox/router atoms, donor benchmark numbers as verified facts, & Blueprint packaging remediation.

## Reconciliation

- Add only `CTX-039` as `EXPLORATORY`: bounded multi-hop evidence-path recall.
- Gate CTX-039 promotion on `CTX-Q004`, `CTX-Q017`, & `CTX-Q038`; traversal consumes canonical identity/provenance/applicability and never reconstructs or infers edges.
- Strengthen `CTX-004` & `CTX-017` instead of adding provenance or relation-cleanup atoms.
- Finish corpus hygiene inside `MEM-029`; use `CTX-030` for drill-down.
- Make corpus-health/maintenance a second live consumer of `MEM-061`–`MEM-066`; add no scheduler subsystem.
- Reconcile `MEM-013` implementation from `UNKNOWN` to `DELIVERED` because `tools/list` consumes negotiated definitions in production; verification & released qualification remain pending.
- Keep `PUL-037`, `PUL-039`, & `PUL-040` as existing independent atoms; prioritize suppression/cache-prefix implementation.
- Freeze Pull invariant: `PUL-017 eligibility → ranking/selection → PUL-040 placement`. Placement is defense-in-depth and grants no authority.
- Store multi-hop, authority-contamination, MCP surface-cost, & repeated-session/cache evaluations in qualification ledgers, not capability totals.

## Evidence

| Capability | State | Exact source | Exact consumer | Residual |
|---|---|---|---|---|
| MEM-013 | DELIVERED | `engine/crates/membrane-mcp/src/tools.rs:114-156` | `engine/crates/membrane-mcp/src/jsonrpc.rs:14-16` | COMPLETE |

- `engine/crates/cortex-core/src/graph.rs`: arbitrary relation strings, allowed dangling edges, one-hop neighbors/search.
- `engine/crates/membrane-runtime/src/store.rs`: persisted wikilink targets & bounded one-hop recall augmentation.
- `engine/crates/membrane-mcp/src/tools.rs` + `jsonrpc.rs`: negotiated default-one-tool discovery consumed by `tools/list`; generic descriptions remain.
- `docs/canon/membrane.md`: memory sentinel gaps & review-specific scheduler implementation note.
- `docs/canon/pull.md`: suppression, reusable-prefix, & semantic-placement mechanisms remain missing.

Donor measurements remain motivation only; no reported latency, multiplier, or token ratio is promoted as independently verified evidence.

## Comparison disposition

| Atom | Scope | Competitive disposition | Best mechanism | Current evidence | Donor evidence | Gap / action |
|---|---|---|---|---|---|---|
| CTX-039 | EXPLORATORY | NOT_COMMITTED | No proven winner | Current Cortex recall performs bounded one-hop augmentation only. | Donor multi-hop demonstrations are reference motivation; mechanisms & reported measurements were not independently compared in this reconciliation. | Hold exploratory; close CTX-004/017/038 gates, freeze CTX-Q039, then compare under one token/authority/scope contract. |

## Count disposition

`CTX-039` is exploratory, so committed capability count is unchanged. Exploratory capability count increases by one. Implementations, qualifications, & decisions remain non-counted.
