# Signing and update trust

## Scope grants

Membrane signs grants with Ed25519 over a domain-separated canonical payload,
binds a key id, and rejects invalid signatures, expiry, status, or immutable
field changes ([`mcp/scope-grant-v1.mjs`](../../mcp/scope-grant-v1.mjs)). The
tamper and forged-key cases are exercised by
[`mcp/scope-grant-v1.test.mjs`](../../mcp/scope-grant-v1.test.mjs) and
[`tests/adversarial/scope-grant-adversarial.test.mjs`](../../tests/adversarial/scope-grant-adversarial.test.mjs).

## Platform artifacts

- macOS source contract requires exact commit/version, `codesign`, notarization,
  stapling, and matching SHA-256 receipt fields; the script never publishes
  ([`docs/release/platform-acceptance.md`](../release/platform-acceptance.md)).
- Windows source contract binds installer hash to a 40-character commit, uses
  Azure Artifact Signing with SHA-256/RFC3161, and verifies with `signtool /pa
  /tw`; receipt validation requires signature, install, update, and uninstall
  gates ([`docs/release/platform-acceptance.md`](../release/platform-acceptance.md)).
- Signed bytes, notarized artifacts, and clean-host execution receipts are
  **unavailable here**; source contracts alone are not release acceptance.

## Update transaction

The update path requires finite quiesce, staging verification, atomic directory
activation, migration, rollback on deterministic failure, and last-step atomic
receipt publication ([`engine/crates/membrane/src/update.rs`](../../engine/crates/membrane/src/update.rs), [`docs/design/update-dual-signature.md`](../design/update-dual-signature.md)). A
remote update channel, key-rotation ceremony, and rollback drill receipt are
**unavailable**.
