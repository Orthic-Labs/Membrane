# Membrane Privacy Statement

This statement is the binary-distribution entry point for the canonical,
user-facing privacy contract in `docs/privacy.md`.

Membrane's provenance adapter captures metadata about a host-driven read,
never the read payload itself. It records no user name, email, IP address,
device fingerprint, file body, or network payload.

When a caller supplies one, a provenance row persists that read's scope-grant
token or digest verbatim. Membrane does not synthesise a scope grant. A
scope-grant value may be credential-shaped or secret, so provenance data must
be treated as sensitive local data; it is not a redacted credential field.

The canonical contract documents recorded fields, local storage, retention,
audit, wipe, and versioning. If this statement and runtime behaviour disagree,
the runtime is the source of truth and the canonical contract must be updated
in the same change.
