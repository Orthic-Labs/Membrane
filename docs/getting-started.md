# Membrane in five minutes (MBR-1002)

This path ends only with a receipt-backed packet. A packet without a receipt is a failed run.

Two paths are documented. Run the **offline fixture** at the bottom first — zero
prerequisites, deterministic, proves the receipt-or-nothing contract in under a
minute. Steps 1–6 below are the **live path**: it drives the real
`membrane_context` MCP tool instead of a fixture, and needs a working checkout
and a native client.

Live-path prerequisites, stated up front so no step below fails silently:

- This exact Membrane checkout, in place — `mcp/install.mjs init`/`install`
  resolve installation binding by walking up from the target root for a
  `tools/lib/memory/runtime.json` service descriptor. That file lives in the
  parent Orthic Labs workspace, not inside a standalone clone of this
  repository; see [README's "Repository posture"](../README.md#repository-posture).
  Outside that workspace, enrollment (`init`, non-dry-run) and activation
  (`install`) fail closed with `installation binding unavailable`, not a
  partial or misleading success.
- One native MCP client already on `PATH`: `codex` or `claude`. This is the
  client's own CLI, used directly against this source checkout — it is a
  different thing from the packaged installer's platform/client support tier.
  That tier is generated, not promised here: see
  [docs/support-matrix.md](support-matrix.md), currently 0 of 10
  platform/client pairs qualified. Nothing below claims otherwise.

## 1. Install prerequisites (0:00)

Use Node 20+ & pnpm 11:

```sh
pnpm install
node --version
pnpm --version
```

`pnpm install` is required before step 4: the native client spawns
`node mcp/server.mjs`, which imports the `@modelcontextprotocol/server`
package from `node_modules`. Steps 2–3 (`mcp/install.mjs`) use only Node
built-ins and need no install.

## 2. Enroll repository (0:45)

From repository root, choose stable IDs & a private registry location:

```sh
export MEMBRANE_PROJECT_REGISTRY="$PWD/.membrane/project-registry.json"
node mcp/install.mjs init "$PWD" --repository demo-repo --scope demo-scope
```

Read JSON output. It must name `repository_id`, `scope_id`, & canonical `root`.

## 3. Activate client (1:30)

Start service, then install native MCP entry. Activation is receipt-verified by a follow-up `mcp get`:

```sh
node mcp/install.mjs install "$PWD" --client codex
# or: node mcp/install.mjs install "$PWD" --client claude --claude-scope project
```

Keep returned install JSON. It is activation evidence, not a packet receipt.

## 4. Request first packet (2:30)

Ask client to call `membrane_context` for this exact caller:

```json
{"task":"orient me","repository":"$PWD","caller":{"root":"$PWD","repositoryId":"demo-repo","scopeId":"demo-scope"}}
```

Accept output only when `packet` is present, `receipts` is non-empty, & every receipt binds to this repository/scope. Save output as `first-packet.json`.

## 5. Read receipt (3:30)

The response carries `receipts` (the packet's provenance list) alongside
`providerStatus` and `degradationReason`. `degradationReason: "none"` is the
only value that means nothing was forced degraded; any other value means the
packet (if present at all) is short of full delivery, and step 4's acceptance
rule already refused a response where `packet` is absent.

## 6. Force degradation (4:30)

Stop the loopback service, or point `CRYPT_PORT` at a port nothing is
listening on (must be 1024–65535 — the client silently falls back to the
default 47851 outside that range), then repeat the step 4 request:

```sh
export CRYPT_PORT=59991   # any port confirmed idle on this machine
```

Expected result is `providerStatus: "unavailable"`,
`degradationReason: "planner_unavailable"`, and **`packet: null`** — the
client fails closed on transport failure rather than fabricating partial
content. Never present degraded output as context success.

## Deterministic offline fixture

Run without a service or native client:

```sh
node docs/examples/quickstart/run.mjs
node docs/examples/quickstart/run.mjs --degraded
```

Fixture exits non-zero when service marker or receipt is missing and labels all output `executionMode: fixture` plus `evidenceAuthority: synthetic`; it never proves live delivery.
