# Membrane in five minutes (MBR-1002)

This path ends only with a receipt-backed packet. A packet without a receipt is a failed run.

The live path uses the signed Windows install. Membrane Hub owns the resident
runtime; the MCP client launches only the installed native `membrane` binary.
Node & Python are development/test tooling, never installed runtime dependencies.

## 1. Install & launch (0:00)

Install the signed Windows Membrane Hub release, then launch **Membrane Hub**.
Wait until Hub reports Membrane **Running**. Hub off means Membrane is unavailable;
no client or sidecar may start a replacement resident service.

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

`membrane stdio-mcp` is a bounded client process. It talks to active Hub; it
does not own Membrane lifecycle or durable storage.

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

With Hub running, Blueprint's installed capability is available through runtime
shipped by Membrane installer. Watcher freshness is Hub-coupled: watcher runs
only while Membrane runs. An unenrolled repository is `not_configured`; stale or
incomplete graph evidence is `degraded`; only a live transport/service failure is
`blueprint_unavailable`.

## 6. Prove fail-closed lifecycle (4:30)

Exit Membrane Hub, then repeat request. Expected native result is:

```json
{"kind":"membrane_unavailable","reason":"hub_inactive","retryable":true}
```

Client must not fabricate context or start an embedded/one-shot Membrane
fallback. Blueprint remains independently usable only as a bounded one-shot
operation; its watcher is not resident while Hub is off.

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
node docs/examples/quickstart/run.mjs
node docs/examples/quickstart/run.mjs --degraded
```

Fixture exits non-zero when service marker or receipt is missing & labels all
output `executionMode: fixture` plus `evidenceAuthority: synthetic`; it never
proves live delivery.
