# Local data and egress inventory

## Local data

| Data | Location / lifetime | Evidence |
| --- | --- | --- |
| Provenance JSONL metadata | `<MEMBRANE_DATA_ROOT>/provenance.jsonl`; append-only; lifetime of install unless user wipes | [`docs/privacy.md`](../privacy.md) |
| Scope-grant token or digest | Provenance row only when caller supplies it; treated as sensitive | [`PRIVACY.md`](../legal/PRIVACY.md), [`docs/privacy.md`](../privacy.md) |
| Workspace-relative dirty paths and diff counts | Same provenance row; file bodies excluded | [`engine/crates/membrane-runtime/src/provenance.rs`](../../engine/crates/membrane-runtime/src/provenance.rs) |
| Installation, binding, and update receipts | Runtime data root; schema/versioned JSON receipts | [`engine/crates/membrane-runtime/src/installation_manifest.rs`](../../engine/crates/membrane-runtime/src/installation_manifest.rs), [`engine/crates/membrane/src/update.rs`](../../engine/crates/membrane/src/update.rs) |

## Egress

- Provenance adapter invokes only read-only `git` commands and opens no
  network socket ([`docs/privacy.md`](../privacy.md)); this is source evidence,
  not a network-monitoring receipt.
- MCP client and federation use loopback HTTP (`127.0.0.1`) as declared by
  [`mcp/client.mjs`](../../mcp/client.mjs) and [`mcp/installation-binding.mjs`](../../mcp/installation-binding.mjs).
- Release tooling may contact Apple notarization or Azure signing services;
  those are build-time integrations, not context-data destinations
  ([`docs/release/platform-acceptance.md`](../release/platform-acceptance.md)).
- A complete runtime egress inventory, firewall capture, and third-party
  provider audit are **unavailable**. No claim of zero egress beyond cited code
  is justified.

## User controls

Audit the JSONL file directly; uninstall removes the data root. The commands and
retention limits are documented in [`docs/privacy.md`](../privacy.md#6-user-rights).
