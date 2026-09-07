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

## Blueprint discovery

All explicit Membrane operations must remain available independently of Hub across Pull, Blueprint, Cortex, Ledger, Adapt & Push. Hub owns automatic background processes only. Do not require agents to start Hub for explicit graph reads, builds, refreshes or other supported operations. Bounded execution retains authorization, freshness & generation/schema checks.

`membrane_blueprint` is included in default native MCP discovery so clients without custom toolset metadata can use repository graph operations. Call `architecture` first with an authorized repository/caller binding; use returned node identities with `symbol`, `references`, or `impact`. Tool discovery alone does not prove provider readiness.
