# `summarize` — Hash-bound source section summary prompt

MBR-303 introduced the canonical Membrane MCP workflow prompts. This document
describes the `summarize` prompt, exposed through the `prompts/get` JSON-RPC
method on both the native (Rust) and legacy (JS) MCP servers.

## What it does

`summarize` reads a single hash-bound DocReadV1 section through
`membrane_source_read` and asks the model to produce a concise summary. The
caller must already hold the expected `contentSha256` from the upstream
DocReadV1 reference; the operation fails closed on mismatch with the typed
`source_read_hash_mismatch` error.

## Operations invoked

| Operation | Purpose |
|---|---|
| `membrane_source_read` | Hash-bound DocReadV1 section fetch for one exact caller binding. |

## Authority scope

- `authorityScope`: `read-only`
- `authorityEscalation`: `false`

`membrane_source_read` never persists anything and never escalates authority.
The prompt cannot widen the caller's grant because it does not invoke any
write-proposed operation.

## Example invocation

```json
{
  "jsonrpc": "2.0",
  "id": 1,
  "method": "prompts/get",
  "params": {
    "name": "summarize",
    "arguments": {
      "repository": "/path/to/repo",
      "sourceRef": "engine/crates/membrane-protocol/src/types.rs:1-60",
      "anchorId": "anchor-protocol-types",
      "expectedContentHash": "sha256:0000000000000000000000000000000000000000000000000000000000000000"
    }
  }
}
```

The server returns a `messages` array with one `user` message instructing the
client to call `membrane_source_read` and summarize the section, surfacing
any typed error verbatim.

## Source of truth

The canonical JSON definition lives at
[`schemas/registry/prompts/summarize.v1.json`](../../schemas/registry/prompts/summarize.v1.json).
