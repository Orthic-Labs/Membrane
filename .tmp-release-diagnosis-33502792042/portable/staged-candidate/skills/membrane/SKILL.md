---
name: membrane
description: Retrieve smallest useful current repository context, source sections, durable memory, & context receipts through Membrane MCP.
---

# Membrane

Membrane is a local-first context service. It assembles current code, rules, decisions, & memory for a task under one context budget, then returns receipts for included & omitted sources.

Use it when a task needs repository-grounded context, a hash-bound source section, or durable working knowledge. Do not use it as raw memory CRUD, or to bypass repository-bound access.

## Default MCP tool

- `membrane_context` retrieves a federated context packet for one exact caller binding. Use it for repository-grounded context; do not use it for raw memory CRUD or filesystem access.

Some installations may opt into additional capability groups. Those groups are not part of default callable surface.

Context packets preserve source type, authority, & freshness. Their receipts record sources omitted because they were skipped, timed out, inaccessible, or outside budget.
