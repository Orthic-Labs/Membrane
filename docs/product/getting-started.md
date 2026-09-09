# Membrane in five minutes (MBR-1002)

This path ends only with a receipt-backed packet. A packet without a receipt is a failed run.

Use installed Windows package; every runtime call remains bound to stable
`current`. Visible native tray owns full resident lifecycle through its daemon,
including Blueprint watchers & background work. Hub dashboard is on demand.
MCP client launches only installed native `membrane` binary.
Blueprint is a native installed service; no agent-supplied Node or Python is required.

## 1. Install & launch (0:00)

Install Windows package. Explicit operations work immediately with Hub off.
Launch **Membrane tray** when automatic watchers & background processes are wanted;
wait for **Running** before relying on automatic refresh.

## 2. Configure MCP (0:45)

Point the MCP client at the installed native entrypoint. Repository `mcp.json`
shows the canonical transport:

```json
{
  "mcpServers": {
    "membrane": {
      "type": "stdio",
      "command": "membrane",
      "args": ["stdio-mcp"]
    }
  }
}
```

`membrane stdio-mcp` serves explicit operations through canonical installed owners.
It may reuse active services or execute bounded work with Hub off. It preserves
storage ownership & starts no watcher, scheduler or replacement daemon.

## 3. Request first packet (1:30)

Ask client to call `membrane_context` for exact repository identity:

```json
{"task":"orient me","repository":"C:\\work\\demo","caller":{"root":"C:\\work\\demo","repositoryId":"demo-repo","scopeId":"demo-scope"}}
```

Accept output only when `packet` is present, `receipts` is non-empty, & every
receipt binds to this repository/scope. Save output as `first-packet.json`.

## 4. Read receipt (2:30)

Response carries `receipts` beside provider status & degradation details. Only
a complete packet with bound receipts proves delivery. Material omissions,
timeouts, inaccessible sources, stale evidence, & budget drops remain explicit.

## 5. Check Blueprint state (3:30)

Blueprint is available through installed CLI & default MCP discovery with Hub on or off.
Explicit graph queries initialize missing graphs; refresh reconciles changed sources.
Only automatic watcher refresh requires active tray-owned daemon. Repository authorization,
schema validation & generation checks apply in both modes.

## 6. Verify explicit execution with Hub off (4:30)

Exit Membrane tray, then repeat explicit context, memory & graph requests. Requests
execute through installed subsystem owners, preserving grants, receipts & freshness
checks. No watcher, scheduler or Hub process should start. All six subsystems follow
[this execution boundary](../architecture/execution-lifecycle-boundary.md).
Typed lifecycle states such as `not_configured`, `degraded`, & `blueprint_unavailable`
remain explicit in status responses.

## Source-checkout verification

Repository verification uses Node 20+ & pnpm 11 for development tooling only:

```sh
pnpm install
pnpm test
pnpm test:mcp
```

These commands do not describe installed runtime process tree.

## Deterministic offline fixture

Run without a service or MCP client:

```sh
node docs/reference/examples/quickstart/run.mjs
node docs/reference/examples/quickstart/run.mjs --degraded
```

Fixture exits non-zero when service marker or receipt is missing & labels all
output `executionMode: fixture` plus `evidenceAuthority: synthetic`; it never
proves live delivery.
