# `plan` — Workspace planning workflow prompt

MBR-303 introduced the canonical Membrane MCP workflow prompts. This document
describes the `plan` prompt, exposed through the `prompts/get` JSON-RPC method
on both the native (Rust) and legacy (JS) MCP servers.

## What it does

`plan` drives a bounded workspace federation through `membrane_context`,
returning a per-repository fan-out trace plus the candidate blocks the
federation actually reached. The caller uses the trace to write a written plan
that names the repositories consulted, the candidates per repository, and any
typed omissions.

## Operations invoked

| Operation | Purpose |
|---|---|
| `membrane_context` (scope = `workspace` recommended) | Federated context packet across catalog repos by alias. |

## Authority scope

- `authorityScope`: `read-only`
- `authorityEscalation`: `false`

The prompt uses the same bounded routing primitive a direct `membrane_context`
call uses. Workspace routing is bounded by:

1. The caller's `grant_policy.child_repository_ids` (MBR-003 independent authz).
2. The workspace catalog rebuild on every call (MBR-004 bounded routing).
3. A single absolute monotonic ingress deadline with bounded concurrency
   (MBR-005).

A no-match task abstains with `target_selection_abstained` instead of fanning
out to every catalog repo.

## Example invocation

```json
{
  "jsonrpc": "2.0",
  "id": 1,
  "method": "prompts/get",
  "params": {
    "name": "plan",
    "arguments": {
      "task": "design the next migration phase for the membrane workspace",
      "repository": "/path/to/workspace",
      "scope": "workspace"
    }
  }
}
```

The server returns a `messages` array with one `user` message instructing the
client to call `membrane_context` and produce a written plan from the
returned `repos[]`, `candidates`, and typed `omissions`. No write-proposed
authority is requested.

## Source of truth

The canonical JSON definition lives at
[`schemas/registry/prompts/plan.v1.json`](../../../schemas/registry/prompts/plan.v1.json).
