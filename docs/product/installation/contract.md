# Installation manifest and IPC handshake contract

**MBR-105** defines how a Membrane installation identifies itself to every
peer that talks to it. The contract is the typed shape
[`InstallationManifestV1`](../../../engine/crates/membrane-protocol/src/installation.rs),
the JSON Schema
[`installation-manifest.v1.schema.json`](../../../schemas/installation-manifest.v1.schema.json),
and the handshake gate
[`handshake_ingress`](../../../engine/crates/membrane-runtime/src/serve.rs) that
the resident service runs on every loopback request.

## Why a manifest

A Membrane installation is one machine-bound runtime with one database, one
identity file, one set of components, and one workspace. Every IPC peer
(loopback HTTP and stateless CLI/MCP transports) needs to know it is talking to
**that** installation and not a sibling — including the case where a
restarted resident picks up the same identity file but advances the active
startup generation. The manifest makes the binding explicit, machine-checked,
and rejectable.

## What the contract rejects

The handshake gate rejects exactly three classes of mismatch:

| Variant | Trigger |
|---|---|
| `wrong-installation` | `installationId` or `startupGeneration` differs from the active identity. Catches clone, copy, rotate, and cross-machine misuse. |
| `incompatible-schema` | `protocolSchemaVersion` or any `components[].digestSha256` differs from the active build. Catches stale clients and mixed-version handshakes. |
| `unexpected-data-root` | `dataRoot` differs after canonicalization. Catches a process started against one workspace being asked to serve another. |

The handshake never blocks a request that did not opt in by sending the
`X-Membrane-Manifest` header. Legacy clients pass through unchanged; the
gate is explicit, not implicit.

## Resident lifecycle

1. **Startup** — `run_service` calls
   `prepare_runtime_identity` to mint or advance the
   `InstallationIdentity` and `StartupClaim`, then builds an
   `InstallationManifestV1` from the identity, the data root, and the
   in-process component list, and publishes it via
   `installation_manifest::publish_active_manifest`. The manifest is then
   immutable for the life of the process.
2. **Steady state** — every loopback request that carries
   `X-Membrane-Manifest` is compared against the published manifest by
   `handshake_ingress`. A mismatch produces
   `400 Bad Request` (header malformed) or
   `421 Misdirected Request` (handshake rejected) and a typed reason
   string suitable for log scraping.
3. **Restart** — `prepare_runtime_identity` advances the
   `startupGeneration`; the next startup publishes a new manifest with
   the same `installationId` and a higher `startupGeneration`. Peers that
   present the previous generation are rejected with
   `wrong-installation`.

## Component digest

The component manifest names the crates that compose the resident build
(`membrane-protocol`, `membrane-runtime` for now). The
`digestSha256` field is computed from each component's name + crate
version. When a release pipeline produces reproducible signed binaries,
this is replaced with the signed binary's digest; the manifest shape is
unchanged.

## Debug surface

The runtime exposes the active manifest through
`installation_manifest::debug_active_manifest` which returns the typed
manifest alongside its canonical JSON form. The canonical form is the
exact byte sequence a peer would send on the wire, so it is the most
useful debugging surface: paste it into the `X-Membrane-Manifest`
header to simulate a peer, or feed it into `verify_handshake` to
reproduce a reject. The HTTP middleware is the production gate;
nothing in the runtime changes based on debug output.

## Wire compatibility

The canonical JSON form (sorted keys, no whitespace) is the same wire
form the TypeScript binding emits. The Rust round-trip test in
`engine/crates/membrane-protocol/tests/installation_handshake.rs` pins
the digest so a future change to either side is caught immediately.

## What is not in this contract

- **Lifecycle authority** — Hub's inherited-stdio `ResidentHelloV1` handshake
  is separate from installation-manifest presentation. The manifest is a
  peer-presentation contract; lifecycle control uses exact fence and
  capability frames.
- **Per-request scope grant** — the `ScopeGrantV1` shape is the
  per-task authority. The manifest is installation-scoped, the grant is
  task-scoped.
- **Schema migration** — bumping `PROTOCOL_SCHEMA_VERSION` is the
  controlled point at which the `incompatible-schema` reject path
  activates. The manifest is the gate; the migration policy lives
  elsewhere.
