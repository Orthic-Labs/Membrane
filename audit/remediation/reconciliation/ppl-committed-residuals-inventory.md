# Pull / Push / Ledger committed residual inventory

Baseline: `559787c9b8552a00df8251252e5ca226d5d57d4d` on 2026-09-08. Canon contracts are `docs/canon/pull.md`, `docs/canon/push.md`, & `docs/canon/ledger.md`. This inventory performs no donor audit.

## Reservation boundary

- Sole integration, check, stage, commit, & push owner: Membrane task `01a07e9f-8f38-7463-b35e-f6b6013bb23e`.
- Current Pull requirements/publication, H9/H10, native/Blueprint, Hook, Hub/package, manifest, Cargo/lock, docs-canon, release, & qualification reservations stay excluded.
- Frozen Push recovery paths (`push/recovery.rs`, `push_hardening.rs`, `push_end_to_end.rs`) stay with current integrator qualification.
- Frozen Ledger diagnostics paths (`ledger/diagnostics.rs`, `ledger/service.rs`, `live_diagnostics_service.rs`, `diagnostic_bundle.rs`, `ledger_owner_acceptance.rs`, `ledger_integrity.rs`, `live_diagnostics_audit_persistence.rs`) stay with current integrator qualification. `ledger/service.rs` may transfer only after that owner explicitly releases it.
- Workers use Luna, edit allowlists only, & run no Cargo, tests, builds, generators, installs, commits, pushes, or merges.

## Residual accounting

| Set | Committed atoms | Existing implementation authority | Named qualification |
|---|---|---|---|
| Pull acquisition/providers | PUL-004–018 | Canon implementation rows PUL-I004–I018: federation registry/scheduler/deadline, provider adapters, normalization, runtime admission, freshness/merge | `pull-residual-qualification`: public staged acquisition, bounded concurrency/cancellation, each provider class, normalization, admission, authority/freshness independence |
| Pull corrective/fusion/budget | PUL-020–023, PUL-025–030, PUL-033 | Canon rows PUL-I020–I023, I025–I030, I033: corrective, merge/RRF/dedupe, core lane/budget/reconcile, runtime publication fence, abstention | `pull-residual-qualification`: one corrective attempt, deterministic merge/dedupe, faithful budget, final policy/resolver fence, typed no-evidence result |
| Pull placement/native scope | PUL-035–036, PUL-040–042 | Canon rows PUL-I035–I036, I040–I042: source resolution, runtime Pull fence, placement, native workspace aggregation, planner/MCP delivery | `pull-residual-qualification`: post-admission placement, multi-root budget/identity/omissions, request-time ceiling, immediate pre-emission refusal. Active publication/native sources remain read-only until current owners land. |
| Push capture/recovery | PSH-001–002, PSH-005, PSH-009, PSH-022–025, PSH-028–029 | Canon rows PSH-I001–I002, I005, I009, I022–I025, I028–I029: `runc`, shared restore/identifier, source read, recovery/API/delivery | Existing frozen recovery lane owns source repair. `push-residual-qualification` names released-path capture, exact restore, proof negotiation, expiry, cleanup, quota, & no-reexecution qualification without editing frozen files. |
| Push representations/fidelity | PSH-003–004, PSH-006–008, PSH-010–017, PSH-019, PSH-026–027 | Canon rows PSH-I003–I004, I006–I008, I010–I017, I019, I026–I027: AST/compress/prep/selection/fidelity/telemetry/egress plus public delivery adapters | `push-residual-qualification`: protected fidelity, refusal, order, typed units, final-wire economics, transport shape, resolver-capability requirement, & outcome joins |
| Push governed adapter | PSH-018, PSH-020–021 | Canon rows PSH-I018, I020–I021: validated `runc` adapter exists; normal CLI wiring remains partial | `push-residual-qualification`: argv boundaries, path confinement, environment stripping, cancellation, one execution, & explicit unsupported-command refusal. Source repair remains separate after qualification identifies exact defect. |
| Ledger core source/index/resolve | LDG-001–013, LDG-015–022, LDG-024–027, LDG-030 | Canon rows LDG-I001–I013, I015–I022, I024–I027, I030: document spine/policy/reconcile/index/outline/query/resolve/erasure/projections/provider/session/identifier | `ledger-residual-qualification`: enrolled-root source reconciliation, single parse, stable identity, exact search-to-resolve, pagination, graph bounds, lifecycle/erasure, literal lane, projection eligibility, & provider authority |
| Ledger format ingestion | LDG-028 | `ledger/document_conversion.rs` converts plain text, JSON, HTML, PDF, & DOCX; `ledger/doc_spine.rs` persists raw/normalized hashes, converter/version/config, losses, & omissions. Existing use is internal; no generic public ingestion operation exists. Media is excluded; `Other` is unsupported. | `ledger-public-format-ingestion`: real CLI + native MCP operation, explicit grant/size bounds, raw/normalized hashes, converter/version/config drift, typed structure/loss/omission, unsupported input, repeated conversion, search→resolve, snapshot/live behavior |
| Ledger diagnostics | LDG-014, LDG-029, LDG-031 | Canon rows LDG-I014, I029, I031: related/backlinks/manifests/drift in `ledger/diagnostics.rs` | Existing frozen Ledger diagnostics lane owns implementation & qualification; this packet dispatches no duplicate edit. |

`LDG-023` is exploratory, not committed, so it is excluded from residual closure.

## Frozen changed-file inventory

Wave A contains every dependency-independent lane. Three qualification lanes are immediately dispatchable:

- `engine/crates/membrane-runtime/tests/pull_residual_qualification.rs`
- `engine/crates/membrane-runtime/tests/push_residual_qualification.rs`
- `engine/crates/membrane-runtime/tests/ledger_residual_qualification.rs`

LDG-028 public ingestion is also assigned to Wave A because it has no dependency on qualification output, but its own activation gate blocks launch until explicit release of `mcp_executor.rs` plus `ledger/service.rs` reservations. It owns native schema, JavaScript MCP bridge, & both schema projections required to make ingestion callable:

- `engine/crates/membrane-mcp/src/tools.rs`
- `engine/crates/membrane-mcp/tests/discovery_roundtrip.rs`
- `mcp/server.mjs`
- `mcp/server.test.mjs`
- `engine/crates/membrane-runtime/src/mcp_executor.rs`
- `engine/crates/membrane-runtime/src/ledger/cli.rs`
- `engine/crates/membrane-runtime/src/ledger/service.rs`
- `engine/crates/membrane-runtime/src/ledger/doc_spine.rs`
- `engine/crates/membrane-runtime/src/ledger/document_conversion.rs`
- `engine/crates/membrane-runtime/tests/ledger_public_format_ingestion.rs`

No other source, test, manifest, fixture, canon, generated, package, or lock file may change. Positive committed behavior must pass; typed unavailability/denial qualifies only genuine negative contracts. Missing positive semantics return exact unclosed defects to integration owner for another bounded packet.
