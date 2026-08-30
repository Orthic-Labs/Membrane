# `checkpoint` — A0 session checkpoint save prompt

MBR-303 introduced the canonical Membrane MCP workflow prompts. This document
describes the `checkpoint` prompt, exposed through the `prompts/get` JSON-RPC
method on both the native (Rust) and legacy (JS) MCP servers.

## What it does

`checkpoint` saves an A0 session orientation checkpoint for the current
caller binding via `membrane_checkpoint_save`. The checkpoint object is
bounded to a `label` and a `summary` field; the prompt explicitly forbids
calling any other write-proposed operation (proposals, feedback, working
context closes, scratchpad writes).

## Operations invoked

| Operation | Purpose |
|---|---|
| `membrane_checkpoint_save` | Save an A0 session checkpoint for one exact caller binding; never durable knowledge. |

## Authority scope

- `authorityScope`: `write-proposed`
- `authorityEscalation`: `false`

The prompt is the only one in the canonical set that sits inside the
write-proposed authority tier, and only because `membrane_checkpoint_save`
itself is a write-proposed operation scoped to the caller's persisted grant.
The prompt grants **no** authority above that single operation — it does not
call:

- `membrane_knowledge_propose` (durable knowledge proposal)
- `membrane_feedback` (receipt-bound outcome feedback)
- `membrane_working_context` with `operation = "close"` (close is destructive)
- `membrane_scratchpad` save or clear

The prompt's `messages` text explicitly forbids those calls so a client
following the prompt cannot accidentally escalate.

## Example invocation

```json
{
  "jsonrpc": "2.0",
  "id": 1,
  "method": "prompts/get",
  "params": {
    "name": "checkpoint",
    "arguments": {
      "repository": "/path/to/repo",
      "checkpointLabel": "after-mbr-303-impl",
      "summary": "Implemented MBR-303: canonical MCP prompts surface, bounded to operations registry."
    }
  }
}
```

The server returns a `messages` array with one `user` message instructing the
client to build a `{ label, summary }` checkpoint object and call
`membrane_checkpoint_save` exactly once with the caller's existing exact
caller binding. The lifecycle receipt must be surfaced verbatim.

## Source of truth

The canonical JSON definition lives at
[`schemas/registry/prompts/checkpoint.v1.json`](../../../schemas/registry/prompts/checkpoint.v1.json).
