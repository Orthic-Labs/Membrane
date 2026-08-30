# Membrane Privacy Statement

This statement is the binary-distribution entry point for the canonical,
user-facing privacy contract in `docs/product/legal/runtime-privacy.md`.

Membrane's provenance adapter captures metadata about a host-driven read,
never the read payload itself. It records no user name, email, IP address,
device fingerprint, file body, or network payload.

When a caller supplies one, a provenance row persists that read's scope-grant
value verbatim; it may be a token or digest. Membrane does not synthesise,
discover, or redact scope grants. A credential-shaped scope grant makes
provenance data sensitive local data.

The canonical contract documents recorded fields, local storage, retention,
audit, wipe, and versioning. If this statement and runtime behaviour disagree,
the runtime is the source of truth and the canonical contract must be updated
in the same change.
