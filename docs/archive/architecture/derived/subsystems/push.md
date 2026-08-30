# Push — Reversible Reduction & Recovery

**Status:** derived subsystem reference · non-normative  
**Canonical name:** Push  
**Parent system:** Membrane  
**Authority:** Membrane canonical doctrine §9 and its implementation phases.

## Purpose

Answer one question:

> **How can the Pull-selected context be made smaller without destroying anything the task may need back?**

Push owns faithful reduction mechanics. Pull/Membrane planner owns evidence
selection, admission, & headroom. Push executes the selected transform & never
becomes a second planner.

## Owns

- One transform/reduction contract.
- Ordered reversible ladder:
  1. exact dedupe;
  2. content-address raw artifact;
  3. deterministic noise removal;
  4. structure-preserving reduction;
  5. extractive faithful reduction;
  6. valid precomputed provenance-bound summary when already available;
  7. resolver-backed reference/metadata;
  8. explicit truncation last.
- Existing reduction lineage remains under Push: `runc`, `skel`, `compress`,
  & `truncate`.
- Content-addressed recoverability artifacts.
- Query-critical protected-span verification and exact restoration.
- Token/byte balance accounting.
- Host-owned adoption telemetry: opportunities, executions, passthrough reasons, bytes/tokens avoided, resolver refetches, restores, failures, task non-regression. Push never opens or writes Cortex storage.

## Does not own

- final ranking/admission — Membrane planner;
- the decision that a piece of evidence deserves attention — Membrane planner;
- durable knowledge — Cortex;
- repository truth — Blueprint;
- document indexing — Ledger.

## Interception points

### A. Tool/MCP result egress

Primary portable integration point before a large result becomes rendered agent context.

### B. Host post-tool rewrite

When a host exposes a capability that can replace/reduce tool output before model consumption, the host adapter routes through the same Push contract.

Adapters capability-probe this behavior. No host-specific rewrite capability is assumed universally.

### C. Source/document reads

Large reads may remain native, be exact-span excerpted, be structure-preserving reduced, or be externalized behind resolver-backed references.

### D. Provider-to-planner acquisition boundary

Providers may bound acquisition and externalize raw artifacts. They do not decide final attention.

### E. Final renderer

Executes only the planner-selected deterministic representation and final size enforcement. It does not invent ranking/policy.

## Protected material

At minimum:

- identifiers;
- exact errors/codes;
- failing test names;
- explicitly requested values;
- cited spans;
- policy/constraint text;
- task entities;
- tool-call/result integrity pairs;
- decision/rationale integrity pairs;
- diff headers/hunks.

If required material is lost, restore it exactly from the raw artifact/resolver or return a typed incomplete/unachievable result.

## Invariants

1. No prompt-critical model call merely to summarize.
2. Savings are never claimed without paired fidelity evidence.
3. Reducer failure falls back toward less reduction/raw/resolver-backed delivery.
4. Verifier uncertainty restores protected source material.
5. Push never becomes a second planner.

## Implementation ownership

The canonical Membrane doctrine currently places Push/artifact/runtime integration under `engine/crates/membrane-runtime/src/push/`.

Public commands are namespaced as `membrane cli push runc|skel|compress|prep|restore`.
Cortex/Persist has no Push transform or transform-telemetry command.

Runtime `cortex-format` remains only an OKF persistence-format dependency,
re-exported through Membrane. Push compression authority is local to the
public `push::compress` budget/rate APIs; no Push reduction path calls its
former Cortex-format compression helper.

Do not create a standalone `push` crate merely to make the subsystem name feel architecturally real. Split it only if an implementation reason independently justifies the boundary.

## Definition of Done

- [ ] One transform contract is exercised on real MCP/tool egress.
- [ ] Large source/document reads use the same reduction/recovery semantics where applicable.
- [ ] Host post-tool rewrite is used only after capability probing.
- [ ] Zero protected corruption on frozen fixtures.
- [ ] Raw evidence remains resolvable.
- [ ] Token/byte balance invariants are property-tested.
- [ ] Push adoption is measured on real eligible events.
- [ ] Task quality does not regress against raw control.
