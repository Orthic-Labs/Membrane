---
name: membrane
description: Retrieve smallest useful current repository context, source sections, durable memory, & context receipts through Membrane MCP.
---

# Membrane

Membrane is a local-first context service. It assembles current code, rules, decisions, & memory for a task under one context budget, then returns receipts for included & omitted sources.

Use it when a task needs repository-grounded context, a hash-bound source section, or durable working knowledge. Do not use it as raw memory CRUD, or to bypass repository-bound access.

## Native Codex & Claude tools

The public native registry advertises two stable verbs:

- `pull` retrieves unified, grant-aware task context from Pull providers, including Blueprint, Cortex, & Ledger evidence.
- `push` stores submitted UTF-8 body bytes as immutable Cortex source memory. Preserve body exactly; it is a durable write.

Legacy `membrane_*` verbs remain accepted by the native bridge for installed compatibility but are not advertised to agents. Do not assume optional toolsets are callable: discovery is authoritative. When a host exposes a negotiated legacy capability, use its exact advertised name & schema.

Use verified enrolled caller root, repository ID & scope ID; do not invent these bindings. For `bounded_response`, declare a response budget & omit `remainingContextCeiling` when host capacity is unknown. `host_fit` requires genuine validated host-capacity evidence.

Context packets preserve source type, authority, & freshness. Their receipts record sources omitted because they were skipped, timed out, inaccessible, or outside budget.

## Five subsystems & explicit operations

Membrane owns one planner, transport, installation identity, receipts, scheduling, & host integration. Pull owns retrieval & faithful delivery; Cortex owns durable memory; Blueprint owns repository graph evidence; Ledger owns source-bound document & skill projections; Adapt emits proposals. Public `push` writes agent-authored memory through Cortex.

Use `pull` for combined repository context. Explicit Blueprint, Ledger, Cortex, & diagnostic operations may exist behind installed compatibility discovery; call them only after the host advertises their exact native name. A read does not build or refresh state: repository reads remain nonmutating, while `build` or `refresh` require an explicitly authorized operation.

Explicit operations remain available with Hub off through harness-owned access to the installed shared engine. Tool discovery alone does not prove provider readiness.
