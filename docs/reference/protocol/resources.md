# Membrane MCP resources and templates (MBR-304)

`engine/crates/membrane-mcp` and `mcp/` (legacy JS) expose a bounded set of
read-only MCP **resources** — structured payloads a client can pull without
triggering a tool call. Every resource lives as a single canonical JSON file
under `schemas/registry/resources/*.json` so the native (Rust) server and the
legacy JS server return the exact same bytes. The resources are bounded,
versioned, and gated by typed access grants; reads outside the matching
grant return a typed rejection with no content leak.

## Why resources, not tools

Tools run operations; resources surface state. Membrane resources answer
**"what is bound right now?"** without ever granting write authority:

- `installation-manifest` — what installation is the client talking to?
- `lease-status` — what lease is currently active?
- `operation-registry` — what tools exist and what closed error codes does
  each one expose?
- `resources-index` — what resources exist, what grants does each require,
  and what grant types does the protocol support?

A client uses resources to **prove the surface is bounded** before issuing a
tool call. Tools (`membrane_context`, `membrane_source_read`, …) carry
authority to do work; resources never do.

## Resource wire shape

Every resource declaration carries the same shape:

```json
{
  "schemaVersion": 1,                  // INDEPENDENT envelope version
  "name": "<resource-name>",
  "version": 1,                        // INDEPENDENT resource version
  "uri": "membrane://resource/<name>/v1",
  "mimeType": "application/json",
  "description": "...",
  "accessGrants": ["<grant>"],         // at least one; never empty
  "authorityEscalation": false,        // resources never escalate
  "authorityNotes": "...",
  "template": { "uriTemplate": "...", "arguments": [] },
  "body": { ... }                      // structured payload; never a string
}
```

- `version` is independently advanced per resource: bumping one resource's
  version never moves a sibling.
- `accessGrants` is a non-empty array of grant-type names drawn from the
  closed set listed in `schemas/registry/resources/resources-index.v1.json`
  under `supportedGrantTypes`. Adding a new grant type is an index-level
  change with a corresponding `indexVersion` bump.
- `authorityEscalation` is always `false`. Resources are read-only; they
  cannot widen authority beyond the grant they declare.

## Supported grant types

The closed set of resource-level grant types lives in
`schemas/registry/resources/resources-index.v1.json`:

| Grant              | Issuer                                   | Surfaces                                              |
|--------------------|------------------------------------------|-------------------------------------------------------|
| `installation.read`| Membrane supervisor at install / startup | `installation-manifest`                              |
| `lease.read`       | Supervisor-resident admission gate       | `lease-status`                                       |
| `protocol.read`    | MCP handshake                            | `resources-index`, `operation-registry`              |

A read whose presented `accessGrants` does **not** intersect the resource's
declared `accessGrants` returns a typed `resource_access_denied` rejection.
The resource body is **never** serialized into the response — a denied
request cannot leak content across grants.

## `resources/list`

`resources/list` returns the four canonical resources with their declared
metadata. The native (Rust) and legacy (JS) servers emit byte-for-byte
identical payloads (proved by `tests/mcp-resources/resources.parity.test.mjs`).

```json
{
  "resources": [
    {
      "name": "installation-manifest",
      "version": 1,
      "uri": "membrane://resource/installation-manifest/v1",
      "mimeType": "application/json",
      "description": "...",
      "accessGrants": ["installation.read"],
      "authorityEscalation": false,
      "authorityNotes": "..."
    },
    ...
  ]
}
```

## `resources/read`

`resources/read` takes a `uri` and an `accessGrants` array the caller
presents. A matching read returns the resource metadata + body. A
non-matching read returns a typed `resource_access_denied` envelope:

```json
{
  "result": {
    "error": {
      "kind": "error",
      "code": "resource_access_denied",
      "message": "resources/read requires one of the grants in accessGrants: [lease.read]; caller presented [protocol.read]",
      "retryable": false,
      "details": {
        "resource": "lease-status",
        "requiredGrants": ["lease.read"],
        "presentedGrants": ["protocol.read"]
      }
    }
  }
}
```

The denied envelope enumerates which grants the caller would have needed,
but never carries the resource body, the installation id, the lease body,
or the operation list. A denial cannot leak content.

## Per-resource documentation

### `installation-manifest` (v1)

- **URI:** `membrane://resource/installation-manifest/v1`
- **MIME:** `application/json`
- **Required grant:** `installation.read`
- **Returns:** the active installation manifest — installation identity,
  release generation, components, bound API schemas, data root digest, and
  native platform.
- **Example read:**

  ```json
  {
    "jsonrpc": "2.0",
    "id": 1,
    "method": "resources/read",
    "params": {
      "uri": "membrane://resource/installation-manifest/v1",
      "accessGrants": ["installation.read"]
    }
  }
  ```

- **Bounded by construction:** the resource body is never serialized into
  a denied response. A caller without `installation.read` receives a typed
  rejection; no installation id, release generation, or component digest is
  exposed.

### `lease-status` (v1)

- **URI:** `membrane://resource/lease-status/v1`
- **MIME:** `application/json`
- **Required grant:** `lease.read`
- **Returns:** the active component lease — installation id, release
  generation, data-root digest, issuedAt, expiresAt, and status.
- **Example read:**

  ```json
  {
    "jsonrpc": "2.0",
    "id": 2,
    "method": "resources/read",
    "params": {
      "uri": "membrane://resource/lease-status/v1",
      "accessGrants": ["lease.read"]
    }
  }
  ```

- **Bounded by construction:** the lease body is gated behind `lease.read`;
  a caller presenting only `protocol.read` (or no grant) gets a typed
  rejection. The denial envelope names the required grant so a client can
  upgrade its grant presentation, but the lease itself never crosses the
  boundary.

### `operation-registry` (v1)

- **URI:** `membrane://resource/operation-registry/v1`
- **MIME:** `application/json`
- **Required grant:** `protocol.read`
- **Returns:** the Membrane MCP operation registry — every tool name,
  its independently-versioned `schemaVersion` / `errorVersion`, its schema
  path, and its closed error-code list.
- **Example read:**

  ```json
  {
    "jsonrpc": "2.0",
    "id": 3,
    "method": "resources/read",
    "params": {
      "uri": "membrane://resource/operation-registry/v1",
      "accessGrants": ["protocol.read"]
    }
  }
  ```

- **Bounded by construction:** the registry never leaks write authority —
  `protocol.read` is read-only and the registry contains metadata only
  (names, schema versions, error codes). A caller without the grant gets a
  typed rejection with no operations listed.

### `resources-index` (v1)

- **URI:** `membrane://resource/resources-index/v1`
- **MIME:** `application/json`
- **Required grant:** `protocol.read`
- **Returns:** the resources index itself — every resource the MCP server
  exposes, its bounded MIME type, its independent version, its required
  access grants, and the closed set of supported grant types the protocol
  understands.
- **Bounded by construction:** the index exposes resource metadata only —
  never the body of any resource. It cannot be used to widen authority
  beyond `protocol.read`.

## Templates

A resource whose body is parametric declares its template in the
`template` block. The current four committed resources are zero-argument
templates — the URI is the full key. Adding a parametric resource in a
future book requires:

1. Writing `schemas/registry/resources/<name>.vN.json` with a `template.uriTemplate`
   block whose placeholders are RFC 6570 level-1 variables.
2. Adding an entry to `schemas/registry/resources/resources-index.v1.json` under
   `resources[]`, including the matching `accessGrants` and `version`.
3. Extending the Rust `resources::read_payload` to substitute the URI
   template variables before the access-grant check.
4. Mirroring the same substitution in `mcp/resources.mjs`.
5. Adding a parity assertion to
   `tests/mcp-resources/resources.parity.test.mjs` so native and legacy
   continue to agree on the substituted body.

Templates are not used in Book 1; they are reserved for the
resource-versioning roadmap in Books 2 and 3.

## Source of truth and round-trip

The Rust module lives in `engine/crates/membrane-mcp/src/resources.rs`
(`list_payload`, `read_payload`, `read_result_payload`, `ReadOutcome`,
`NAMES`, `URIS`). The legacy JS module lives in `mcp/resources.mjs`
(`listResources`, `readResource`, `readResourceByName`,
`allResourceDefinitions`, `RESOURCE_NAMES`, `RESOURCE_URIS`).

Both modules load the same four `schemas/registry/resources/*.json` files at
runtime (or compile time, in the Rust case, via `include_str!`) and project
them through the same field order. The parity test
`tests/mcp-resources/resources.parity.test.mjs` proves the projection is
byte-for-byte identical, both under deep-equal and under key-sorted
serialization.

Three test files lock the contract:

- **Rust:** `engine/crates/membrane-mcp/tests/resources.rs` — every
  resource is bounded, versioned, and gated by a known grant; matching
  reads succeed, mismatched reads return a typed rejection with no body;
  unknown URIs return a typed not-found envelope.
- **JS contract:** `tests/mcp-resources/resources.contract.test.mjs` —
  the same four properties, asserted against the legacy JS surface.
- **JS parity:** `tests/mcp-resources/resources.parity.test.mjs` —
  the JS `resources/list` payload equals the canonical projection the Rust
  module emits; when the native binary is available, the live wire payload
  equals the JS projection too.

A drift in any resource's `accessGrants`, `version`, `mimeType`, or body
shape fails **both** the Rust and JS suites. That is the visible contract
story MBR-304 requires: resources are bounded, versioned, and do not leak
content across grants.

## Cross-references

- MBR-301 per-operation contracts: `docs/reference/protocol/operations.md`.
- MBR-101 protocol source of truth: `docs/reference/protocol/source-of-truth.md`.
- MBR-103 native (Rust) MCP hot path: `engine/crates/membrane-mcp/src/`.
- MBR-303 prompt parity precedent (same projection pattern):
  `tests/mcp-prompts/prompts.parity.test.mjs`.
