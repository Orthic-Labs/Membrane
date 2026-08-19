# Push — Reversible Reduction

**Status:** canonical subsystem doctrine · draft for adoption
**Code today:** `runc` / `skel` / `compress` inside the `crypt` crate; compress/spill store in `membrane-runtime` — **misplaced**
**Parent:** `docs/SYSTEM.md` · Membrane doctrine §9

## Purpose
Answer one question: **how do we shrink what is already flowing to the agent without losing anything we might need back?**

## Owns
- One transform contract; the ordered ladder: dedupe → content-address raw artifact → noise strip → structure-preserving reduction → extractive reduction → valid precomputed summary → resolver-backed ref → truncation last.
- `ArtifactRefV1` content-addressed raw store (own store; regenerable by re-capture).
- Query-critical verifier (identifiers, errors, test names, cited spans, policy text, tool-call/result pairs, diff header/hunk) with exact restore.
- `TokenBalanceV1 { original ≥ materialized ≥ delivered, provider_billed }` + typed `skip_reason`.
- Adoption telemetry: opportunities, executions, passthrough reasons, bytes avoided, restores, failures.

## Does not own
Ranking or admission (Planner) · what is delivered (Planner) · memory (Cortex).

## Public contract
Interception points: (A) MCP/tool result egress — primary; (B) host post-tool hook where the host supports result rewriting (Claude Code does not; `additionalContext` only); (C) large source/file reads; (D) provider→planner payload cap; (E) final renderer executes the selected representation only.

## Invariants
1. Never worse than raw; savings never claimed without a paired fidelity assertion.
2. Protected spans survive or restore exactly; otherwise typed `budget_unachievable_with_protections`.
3. No model call in the prompt-critical path to summarize.
4. Artifact write fails → keep raw; reducer fails → less reduction; verifier uncertain → restore.

## Definition of Done
- [ ] Extracted to `engine/crates/push/` before the Crypt→Cortex rename.
- [ ] Wired at MCP egress and source reads; adoption measured on receipts (baseline 1-in-7).
- [ ] Zero protected corruption on fixtures; raw resolvable.
- [ ] `TokenBalanceV1` inequalities property-tested.
