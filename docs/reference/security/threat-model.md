# Threat model

## Assets and boundaries

Assets are repository-root bindings, scope grants, local provenance, update
receipts, and generated context. MCP authorization is the boundary; raw durable
write and filesystem tools are intentionally absent ([MCP threat model](../../architecture/security/mcp-threat-model.md)).
The service is loopback-bound by the installation binding
([`engine/crates/membrane-runtime/src/installation_manifest.rs`](../../../engine/crates/membrane-runtime/src/installation_manifest.rs)); a clean
external-host proof is **unavailable**.

## Adversaries and controls

| Adversary | Control | Traceable evidence |
| --- | --- | --- |
| Cross-root caller | Exact repository binding; no child grant means deny | [`engine/crates/membrane-mcp/src/authorization.rs`](../../../engine/crates/membrane-mcp/src/authorization.rs) |
| Forged or widened grant | Canonical Ed25519 bytes, key id, expiry, immutable fields | [`engine/crates/membrane-mcp/src/scope_grant.rs`](../../../engine/crates/membrane-mcp/src/scope_grant.rs) |
| Prompt injection in source | Source is data; exact range and path validation precede grant minting | [`engine/crates/membrane-mcp/src/scope_grant.rs`](../../../engine/crates/membrane-mcp/src/scope_grant.rs) |
| Corrupt or partial registry | Atomic write, schema failure is fail-closed | [MCP threat model](../../architecture/security/mcp-threat-model.md), [`engine/crates/membrane-runtime/src/installation_manifest.rs`](../../../engine/crates/membrane-runtime/src/installation_manifest.rs) |
| Local journal disclosure | Metadata-only provenance; no payload or socket | [`runtime-privacy.md`](../../product/legal/runtime-privacy.md), [`engine/crates/membrane-runtime/src/provenance.rs`](../../../engine/crates/membrane-runtime/src/provenance.rs) |
| Malicious update | Verified staging, atomic activation, rollback, last-step receipt | [`engine/crates/membrane/src/update.rs`](../../../engine/crates/membrane/src/update.rs), [update admission](../../architecture/security/update-admission.md) |

## Residual risk

Filesystem compromise, compromised signing credentials, and malicious host
processes are outside this model. Independent installed-host, cross-platform,
and external-boundary receipts are **unavailable in this document set**; do not
claim those properties from source tests alone.
