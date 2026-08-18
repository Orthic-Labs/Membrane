# Membrane security

Membrane is an internal, workspace-coupled control plane, not a standalone
public service ([README](README.md#repository-posture)). Security claims below
are limited to checked source, tests, or release contracts; missing proof is
marked **unavailable**.

## Report a vulnerability

Do not put secrets, exploit code, or private data in a public issue. The
repository contains no published security mailbox or disclosure SLA
(**unavailable**). Send a minimal private report to the maintainer/contact that
came with your installation, including affected revision, platform, impact,
reproduction, and a safe contact method. Request acknowledgement before
sharing sensitive details. Do not test against another user's installation.

## Security surface

- Authorization is monotone: installation, caller, target, child, task, and
  operation levels intersect in [`mcp/authorization.mjs`](mcp/authorization.mjs);
  adversarial zero-admission coverage is documented in
  [`docs/security/adversarial-authorization-suite.md`](docs/security/adversarial-authorization-suite.md).
- Scope grants are short-lived Ed25519 signatures with canonical signing bytes;
  implementation and tamper tests are [`mcp/scope-grant-v1.mjs`](mcp/scope-grant-v1.mjs)
  and [`mcp/scope-grant-v1.test.mjs`](mcp/scope-grant-v1.test.mjs).
- Provenance is local metadata only. Recorded fields, exclusions, storage, and
  wipe semantics are in [`docs/privacy.md`](docs/privacy.md) and the adapter
  ([`mcp/adapters/provenance/index.mjs`](mcp/adapters/provenance/index.mjs)).
- Release trust is contract-level until signed artifacts and clean-host receipts
  exist; see [`docs/security/signing-update-trust.md`](docs/security/signing-update-trust.md).

## Supported versions

Node.js `>=20` is the declared runtime floor ([`package.json`](package.json)).
The current MCP protocol and client support claims are generated in
[`docs/clients/support-matrix.v1.json`](docs/clients/support-matrix.v1.json);
unsupported or degraded cells must not be described as fully supported.
There is no separately published security-support end date (**unavailable**).

## Evidence rule

Documentation never upgrades a source-only claim into installed or published
proof. Mac and Windows acceptance requires the receipts described in
[`docs/release/macos.md`](docs/release/macos.md) and
[`docs/release/windows.md`](docs/release/windows.md).
