# `recap` — Session recap workflow prompt

MBR-303 introduced the canonical Membrane MCP workflow prompts. This document
describes the `recap` prompt, which is exposed through the `prompts/get` JSON-RPC
method on both the native (Rust) and legacy (JS) MCP servers.

## What it does

`recap` pulls the active working context for a `(sessionId, taskId)` pair and
combines it with a fresh `membrane_context` packet for the current repository.
The result is a concise summary of where the session currently stands so a
client can resume work without re-deriving orientation state from scratch.

## Operations invoked

| Operation | Purpose |
|---|---|
| `membrane_working_context` (operation = `load`) | List the active working contexts bounded to the supplied `(sessionId, taskId)`. |
| `membrane_context` | Fetch a single-repo federated context packet for the supplied task. |

Both operations are read-only with respect to durable state.

## Authority scope

- `authorityScope`: `read-only`
- `authorityEscalation`: `false`

The prompt is bounded to the caller's existing grant policy. It cannot widen
authority because:

1. `membrane_working_context` with `operation: "load"` does not write to the
   store — it returns active contexts.
2. `membrane_context` is a federation read; the workspace fan-out is bounded
   by the caller's `grant_policy.child_repository_ids`.

## Example invocation

```json
{
  "jsonrpc": "2.0",
  "id": 1,
  "method": "prompts/get",
  "params": {
    "name": "recap",
    "arguments": {
      "sessionId": "sess_a",
      "taskId": "task_alpha",
      "task": "summarize the current orientation",
      "repository": "/path/to/repo"
    }
  }
}
```

The server returns a `messages` array with one `user` message instructing the
client to call `membrane_working_context` and `membrane_context` in order and
summarize the results. No further authority is granted.

## Source of truth

The canonical JSON definition lives at
[`schemas/registry/prompts/recap.v1.json`](../../schemas/registry/prompts/recap.v1.json).
Both the native (Rust) and legacy (JS) servers embed that file via
`include_str!` and runtime `readFile` respectively and serve it verbatim.
