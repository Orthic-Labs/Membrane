# Audit closure inventory

## Packet, baseline, scope

- **status:** complete (read-only inventory; no product closure claimed)
- **baselineRevision:** `2e8e57bbb1e368332b37466080f1e925f2f4726`
- **packet receipt:** verified against `audit/remediation/blueprint-native-reconciliation.dispatch.receipt.json`; packet SHA-256 `8ab6486fa2c3086e9ac9f49d69aa876af9710d2d0f4021b657b24a81f9f990ca`, prompt digest `sha256:030f24925602b59dd7b4557f6f4d1e2dffc94b246c281177b8c24f74e1086bcc`.
- **authority:** dispatch packet plus `AGENTS.md`, `blueprint/AGENTS.md`, canonical Membrane/Blueprint/lifecycle/CodeRight docs, remediation README, & closure brief as named by packet.
- **changed path:** `audit/remediation/reconciliation/audit-closure-inventory.md` only.

## SEM status

The frozen semantic audit is source-only: no product test/build/runtime was run, no Session B artifact was read, & no runtime claim was made (`audit/session-a/MEMBRANE-SEMANTIC-AUDIT.md:3-5`). Its 353-row matrix retains `implementation_unknown:43`, `missing_implementation:15`, `partial_unverified_terminal_path:137`, `source_locus_identified_prior_evidence_only:102`, & `source_locus_identified_unverified:56` (`audit/session-a/MEMBRANE-SEMANTIC-AUDIT-MANIFEST.json:coverage`). Therefore SEM is **open / not a closure gate**.

| Finding | Current status | Exact acceptance still required |
|---|---|---|
| SEM-001 | Source-confirmed; later installed checkpoints claim Hub-off explicit operations, but no native Blueprint parity/native-only installed proof | Installed MCP must return bounded Blueprint one-shot with generation/freshness, no daemon/watcher, while context/Ledger stay `hub_inactive` (`audit/session-a/MEMBRANE-SEMANTIC-AUDIT.md:24-32`). |
| SEM-002 | Source-confirmed; typed absence repair is recorded, qualification remains separate | Missing audit/decision owner must produce explicit omission/incomplete and never complete-empty (`audit/session-a/MEMBRANE-SEMANTIC-AUDIT.md:34-42`). |
| SEM-003 | Source-confirmed open deadline/fan-out defect; source checkpoint is not full closure | One ingress absolute deadline, bounded parallel children, late typed omissions, healthy siblings retained (`audit/session-a/MEMBRANE-SEMANTIC-AUDIT.md:44-52`). |
| SEM-004 | Source-confirmed open | Push recovery must bind verified task + session, with same-task success and wrong-identity denial (`audit/session-a/MEMBRANE-SEMANTIC-AUDIT.md:54-62`). |
| SEM-005 | Source-confirmed open | Unwritable audit sink must return stable `audit_persistence_unavailable`, not silent success (`audit/session-a/MEMBRANE-SEMANTIC-AUDIT.md:64-72`). |
| SEM-006 | Source-confirmed open | Discovery must separate stable schema advertisement from per-owner readiness (`audit/session-a/MEMBRANE-SEMANTIC-AUDIT.md:74-82`). |
| SEM-007 | Unmeasured opportunity | Warm-call counters must prove immutable owner/registry reuse without weakening revocation (`audit/session-a/MEMBRANE-SEMANTIC-AUDIT.md:84-90`). |
| SEM-008 | Source/document mismatch open | Generated truth must distinguish implementation inventory from qualified journey; schema gate must prevent status promotion (`audit/session-a/MEMBRANE-SEMANTIC-AUDIT.md:92-100`). |

## EVID status

- **EVID-001 open:** baseline substitution & independence chronology are unproven; README explicitly says requested `75c842…` baseline was replaced by `a4e0a9…`, self-attestation is not independent provenance (`audit/remediation/README.md:7-11`, finding table at `audit/remediation/README.md:62`). Required evidence is raw authoritative turns plus freeze timeline.
- **EVID-002 open:** Session B reports zero executed product cases and its oracle is only a test specification; named assertions are missing for F01/H01/I01/J01/L02 and environment-variable identity/isolation is not enforced (`audit/remediation/README.md:11`, finding table at `audit/remediation/README.md:63`). Required evidence is boundary-correct assertions for every frozen observable.
- Qualification artifacts are mechanics-only where marked so: Pull and Push explicitly state no task-success evaluator, unavailable latency/host restore instrumentation, and no policy promotion (`docs/evidence/qualification/pull-metrics.json`, `docs/evidence/qualification/push-metrics.json`). The macOS MBR-801 receipt is incomplete with zero scenarios and no installed execution (`docs/evidence/qualification/mbr801/macos/receipt.json`).

## CI / release status

- **CI-001:** runtime inventory was regenerated and local checks passed, but CI/release qualification remains distinct (`audit/remediation/README.md:64`). The historical release manifest still has 4 `boundedExternalInterpreterRows`, including packaged Blueprint Node/scripts/watchman and launchers (`docs/evidence/releases/92d72b80-windows-x86_64/runtime-language-manifest.json:19-23,213-247,292-305,470-483`).
- **N8:** Blueprint packaging/runtime-boundary implementation landed, but installed Hub-hosted/one-shot receipts remain pending (`migration/native-rust/MEMBRANE-NATIVE-RUST-MIGRATION-AND-CODERIGHT-INTEGRATION.md:986-1000`). **N10:** native-only seal is blocked pending N2/N5-N9 receipts and Section 17 package/SBOM/process-tree/native-only gates (`...INTEGRATION.md:1023-1034`; current cutover summary `migration/native-rust/README.md:25-30`).
- Existing `migration/native-rust/native-only-seal.json` is not evidence of a newly qualified Blueprint-native artifact; exact package-digest qualification, interpreter-absent installed runs, process-tree proof, SBOM/package proof, and closure receipt remain required by the migration gate (`...INTEGRATION.md:1351-1436`).

## Missing acceptance, overlap, & dependencies

1. **Native Blueprint migration dependency:** current native surface is only `engine/crates/membrane-blueprint/src/lib.rs`; production runtime still has Blueprint IPC/one-shot seams (`engine/crates/membrane-runtime/src/blueprint_one_shot.rs`, `mcp_executor.rs:1181-1254`) and federation provider binding (`engine/crates/membrane-federation/src/providers/blueprint.rs`, `blueprint_client.rs`). Native implementation must preserve generation/freshness, schema-v20 SQLite compatibility, grants/confinement, Phase 2, watcher reconciliation, MCP, CLI, & federation semantics before deleting `blueprint/{src,scripts,watchman}`.
2. **SEM overlap:** SEM-001/003/006 consume the same runtime dispatch/deadline/readiness boundaries; SEM-002 consumes federation provider accounting; SEM-004 consumes Push recovery; SEM-005 consumes diagnostics persistence. Fix/qualify those seams alongside native Blueprint cutover, but do not treat source tests as installed closure.
3. **Required installed acceptance:** exact immutable installer digest, Node/Python absent from `PATH`, Blueprint init/build/query/search/neighborhood/freshness/watcher/Phase 2/MCP/federation, Hub lifecycle, restart, process-tree, package/SBOM, upgrade/rollback, and no legacy selector reachability. This is explicitly required by migration Sections 16-17 and absent from current receipts.
4. **Evidence dependency:** preserve initial/final Session A artifacts; obtain independent baseline chronology & boundary-correct Session B execution; produce per-atom terminal evidence rather than promoting static loci. The audit says uninspected Blueprint extractors/interleavings, installed artifact identity, CLI/SDK/HTTP parity, rollback, & resource measurements remain unverified (`audit/session-a/MEMBRANE-SEMANTIC-AUDIT.md:114-118`).

## Exact later file proposals (not edited in this lane)

| Later slice | Exact proposed files |
|---|---|
| Native Blueprint core | `engine/crates/membrane-blueprint/src/lib.rs`; add `contracts.rs`, `store.rs`, `discovery.rs`, `parsers.rs`, `graph.rs`, `freshness.rs`, `watcher.rs`, `phase2.rs`, `grants.rs`, `service.rs`, `mcp.rs`, `cli.rs`; add crate unit/integration tests under `engine/crates/membrane-blueprint/tests/`. |
| Runtime seams & federation | `engine/crates/membrane-runtime/src/blueprint_one_shot.rs`, `mcp_executor.rs`, `pull/federation_sources.rs`, `pull/native_federation.rs`; `engine/crates/membrane-federation/src/blueprint_client.rs`, `providers/blueprint.rs`, `engine.rs`, `deadline.rs`; corresponding `engine/crates/membrane-runtime/tests/` and `engine/crates/membrane-federation/tests/provider_blueprint.rs`. |
| Security, readiness, Push, diagnostics | `engine/crates/membrane-runtime/src/authorization.rs`, `hub.rs`, `push/api.rs`, `push/recovery.rs`, `live_diagnostics_service.rs`, `diagnostic_bundle.rs`; tests `authorization_conformance.rs`, `hub_snapshot_cli_contract.rs`, new recovery identity/readiness/audit-persistence tests. |
| Packaging/lifecycle/cleanup | `apps/membrane-hub/src-tauri/windows/installer.nsi`, release manifests under `apps/membrane-hub/src-tauri/`, `scripts/release/`, `blueprint/release/`; remove retired Blueprint executable runtime/launchers only after parity receipts. |
| Policy, graph, evidence | `migration/native-rust/runtime-policy.json`, `runtime-language-manifest.json`, `invocation-graph.json`, `native-only-seal.json`, `executable-ledger.json`, `docs/evidence/releases/<exact-digest>/runtime-language-manifest.json`, `docs/evidence/releases/<exact-digest>/sbom.json`, `docs/evidence/qualification/mbr801/<platform>/receipt.json`, & final closure report. |
| Canonical docs | `docs/architecture/subsystems/blueprint.md`, `docs/architecture/execution-lifecycle-boundary.md`, `docs/architecture/integrations/coderight.md`, generated product/runtime truth through their owning generators. |

## Commands / deviations / next

- **commands:** read-only `Get-Content`, `rg`, JSON inspection; no tests, builds, generators, installs, Cargo, watcher activation, graph rebuild, commits, pushes, merges, or heavy checks (per worker policy).
- **deviations:** none; no files outside allowlist touched.
- **next:** integration owner verifies citations/completeness, then authors bounded native Blueprint implementation/qualification packets and obtains independent SEM/EVID/CI closure receipts before changing policy or deleting JS authority.
