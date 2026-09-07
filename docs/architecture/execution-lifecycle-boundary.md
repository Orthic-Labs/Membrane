# Explicit execution & resident lifecycle

**Status:** Normative user correction, 2026-09-07. Source implementation landed; native CI & installed acceptance are tracked in `audit/remediation/README.md`.

This contract supersedes any statement that tray-off or Hub-off disables explicit Membrane operations, that MCP/CLI must only forward to a daemon, or that Blueprint is the only subsystem permitted bounded execution. It applies to all six subsystems & every installed agent integration, including CodeRight.

Hub owns automatic background execution & resident process lifetime. It does not own availability of explicitly requested operations. An agent must be able to invoke any supported explicit Membrane operation with Hub stopped.

| Subsystem | Explicit operations available with Hub off | Automatic work requires Hub |
|---|---|---|
| Pull | Context retrieval, planning, fusion & receipts | Scheduled retrieval or prewarming |
| Blueprint | Graph inspection, initial build, refresh, rebuild, search, traversal, analysis, verification & export | Auto-refresh watchers & scheduled analysis |
| Cortex | Recall, memory operations, checkpoints & authorized durable writes | Automatic consolidation or maintenance |
| Ledger | Registration, indexing, search, navigation & source resolution | Watchers & scheduled reindexing |
| Adapt | Inspection, feedback & authorized proposal operations | Automatic observation, review & proposal generation |
| Push | Explicit prepare, reduction & resolution | Automatic interception or background reduction |

Explicit execution uses installed product entry points, bounded process lifetime & existing subsystem services. It preserves repository authorization, caller identity, grants, freshness, generation/schema validation, transaction semantics & concurrency control. It never starts Hub, installs a service, enrolls a watcher or leaves a resident process behind. This contract does not authorize bypassing another subsystem's storage owner or protected effects.

With Hub active, requests may reuse resident services. With Hub stopped, equivalent explicit requests execute on demand. Automatic subscriptions report inactivity when Hub is stopped; ordinary explicit requests must not fail solely with `hub_inactive`.

Diagnostics workspace epochs, mutations, snapshots & baselines persist through canonical Cortex event storage. Every CLI, MCP & resident diagnostics owner reads that same versioned state; revision conflicts & corrupt payloads fail closed. Provider handles remain process-local. Explicit acquisition shuts providers down before returning; subscriptions & resident provider restart require Hub. Loopback failure after dispatch never causes a mutation replay.

## Regression acceptance

For each installed public operation, verify discovery & execution with Hub on & off. Compare semantic results & authorized effects against identical input state. Include first-use initialization, changed-source refresh, durable writes followed by reads, concurrent requests, cancellation & process exit. Retain negative tests for authorization, schema/generation mismatch & storage consistency.

Separately verify that stopping Hub stops every watcher, scheduler & automatic process while explicit operations remain callable. Tests that expect blanket Hub-off refusal for ordinary operations encode obsolete behavior & must be replaced.

## Enforced ownership sites

- `engine/crates/membrane-runtime/src/mcp_executor.rs`: request/session owner executes explicit operations when Hub is inactive; dispatched writes are never replayed after uncertain transport failure.
- `docs/architecture/membrane.md`: all six subsystems permit explicit bounded execution with Hub off.
- `docs/architecture/subsystems/ledger.md`: explicit indexing uses canonical Ledger owner independently of Hub.
- `docs/architecture/subsystems/adapt.md`, `cross-subsystem-evidence.md` & `integrations/coderight.md`: canonical installed owners serve explicit operations; automatic processes remain Hub-owned.
- `docs/architecture/security/mcp-threat-model.md`: bounded explicit execution preserves authorization & process containment; automatic residency requires Hub.

Blueprint installed accessibility is first delivery priority. This contract records required behavior; it is not evidence that current binaries satisfy it.
