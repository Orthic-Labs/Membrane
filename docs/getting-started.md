# Membrane in five minutes (MBR-1002)

This path ends only with a receipt-backed packet. A packet without a receipt is a failed run.

## 1. Install prerequisites (0:00)

Use Node 20+ & pnpm 11:

```sh
pnpm install
node --version
pnpm --version
```

The live path also needs a running Membrane loopback service plus one native client (`codex` or `claude`) on `PATH`.

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

Receipt explains admission, omissions, provider status, & degradation. `degradationReason: "none"` means no forced degradation; any other reason must appear in `omissions`.

## 6. Force degradation (4:30)

Stop loopback service (or point `MEMBRANE_FEDERATE_URL` at an unused port) & repeat request. Expected result is `providerStatus: "degraded"`, a non-empty `degradationReason`, and **no packet body**. Never present degraded output as context success.

## Deterministic offline fixture

Run without a service or native client:

```sh
node examples/quickstart/run.mjs
node examples/quickstart/run.mjs --degraded
```

Fixture exits non-zero when service marker or receipt is missing and labels all output `executionMode: fixture` plus `evidenceAuthority: synthetic`; it never proves live delivery.
