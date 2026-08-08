# Threat model

## Assets and boundaries

Assets are repository-root bindings, scope grants, local provenance, update
receipts, and generated context. MCP authorization is the boundary; raw durable
write and filesystem tools are intentionally absent ([`docs/THREAT-MODEL-MCP-V1.md`](../THREAT-MODEL-MCP-V1.md)).
The service is loopback-bound by the installation binding
([`mcp/installation-binding.mjs`](../../mcp/installation-binding.mjs)); a clean
external-host proof is **unavailable**.

## Adversaries and controls

| Adversary | Control | Traceable evidence |
| --- | --- | --- |
| Cross-root caller | Exact repository binding; no child grant means deny | [`mcp/authorization.mjs`](../../mcp/authorization.mjs), [`tests/adversarial/authorization-adversarial.test.mjs`](../../tests/adversarial/authorization-adversarial.test.mjs) |
| Forged or widened grant | Canonical Ed25519 bytes, key id, expiry, immutable fields | [`mcp/scope-grant-v1.mjs`](../../mcp/scope-grant-v1.mjs), [`mcp/scope-grant-v1.test.mjs`](../../mcp/scope-grant-v1.test.mjs) |
| Prompt injection in source | Source is data; exact range and path validation precede grant minting | [`mcp/scope-grant-v1.mjs`](../../mcp/scope-grant-v1.mjs), [`tests/adversarial/scope-grant-adversarial.test.mjs`](../../tests/adversarial/scope-grant-adversarial.test.mjs) |
| Corrupt or partial registry | Atomic write, schema failure is fail-closed | [`docs/THREAT-MODEL-MCP-V1.md`](../THREAT-MODEL-MCP-V1.md), [`mcp/installation-binding.test.mjs`](../../mcp/installation-binding.test.mjs) |
| Local journal disclosure | Metadata-only provenance; no payload or socket | [`docs/privacy.md`](../privacy.md), [`engine/crates/membrane-runtime/src/provenance.rs`](../../engine/crates/membrane-runtime/src/provenance.rs) |
| Malicious update | Verified staging, atomic activation, rollback, last-step receipt | [`engine/crates/membrane/src/update.rs`](../../engine/crates/membrane/src/update.rs), [`docs/update.md`](../update.md) |

## Residual risk

Filesystem compromise, compromised signing credentials, and malicious host
processes are outside this model. Independent installed-host, cross-platform,
and external-boundary receipts are **unavailable in this document set**; do not
claim those properties from source tests alone.
