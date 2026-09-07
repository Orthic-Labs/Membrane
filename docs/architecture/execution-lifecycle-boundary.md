# Explicit execution & resident lifecycle

**Status:** Normative user correction, 2026-09-07. Source implementation & installed acceptance are tracked in `audit/remediation/README.md`. Internal Windows delivery uses local RightKit builds & installed checks; CI does not gate that loop.

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

Installation reconciles stable-path MCP bindings & CLI access before resident startup. A failed Hub launch must not remove those explicit entry points. Pull freshness reads use the same bounded Blueprint transport as explicit graph operations; diagnostics preserve enrolled scope descriptors when authorizing CLI calls. Cold Blueprint initialization preserves repository source files unless the caller explicitly requests documentation changes.

Local CLI invocation carries its OS caller's explicit repository scope; watcher enrollment is never its admission list. Remote adapters retain their caller/root authorization. Native explicit builds reuse an available resident owner or fall back to bounded local execution. Findings explanation & evidence packs use their canonical sealed-generation service when no daemon is reachable.

Hub-owned explicit builds & refreshes stop the resident watcher, finish the bounded write, then restart the authenticated watcher. The service parent serializes actual workers through this handoff; cancellation of a client waiter cannot restart a watcher while its shared build still writes. Full builds acquire the canonical store lease before writing any side artifact. Independent Windows requests own separate unnamed Job Objects; terminating one request cannot terminate another, & forced termination never reports success.

Ordinary Pull results need no protected block to be deliverable. An empty protected set preserves its meaning; exact packet measurement, host capacity, evidence lineage & reversible-recovery checks still govern delivery.

One-shot freshness completes deterministic document extraction after applying every source delta, while leaving judgment work explicitly pending. Cold builds & incremental repair use content identity for every indexed file. Replayed identical source events acknowledge journal clocks without reparsing repository symbols. Federation validates Blueprint graph identifiers independently of release artifact identifiers.

## Regression acceptance

For each installed public operation, verify discovery & execution with Hub on & off. Compare semantic results & authorized effects against identical input state. Include first-use initialization, changed-source refresh, durable writes followed by reads, concurrent requests, cancellation & process exit. Retain negative tests for authorization, schema/generation mismatch & storage consistency.

Separately verify that stopping Hub stops every watcher, scheduler & automatic process while explicit operations remain callable. Tests that expect blanket Hub-off refusal for ordinary operations encode obsolete behavior & must be replaced.

## Enforced ownership sites

- `engine/crates/membrane-runtime/src/mcp_executor.rs`: request/session owner executes explicit operations when Hub is inactive; dispatched writes are never replayed after uncertain transport failure.
- `engine/crates/membrane-runtime/src/freshness.rs`: Pull & diagnostics read Blueprint through resident-or-one-shot transport.
- `engine/crates/membrane/src/activation.rs`: explicit agent bindings precede Hub startup; `tests/activation_hub_off.rs` checks startup failure preserves them.
- `docs/architecture/membrane.md`: all six subsystems permit explicit bounded execution with Hub off.
- `docs/architecture/subsystems/ledger.md`: explicit indexing uses canonical Ledger owner independently of Hub.
- `docs/architecture/subsystems/adapt.md`, `cross-subsystem-evidence.md` & `integrations/coderight.md`: canonical installed owners serve explicit operations; automatic processes remain Hub-owned.
- `docs/architecture/security/mcp-threat-model.md`: bounded explicit execution preserves authorization & process containment; automatic residency requires Hub.

Blueprint installed accessibility is first delivery priority. This contract records required behavior; it is not evidence that current binaries satisfy it.
