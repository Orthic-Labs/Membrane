# Adversarial Authorization & Prompt-Injection Suite (MBR-802)

## Purpose

This document describes the threat model behind `tests/adversarial/` and the
**zero unauthorized-admission guarantee** that the release qualification gate
enforces via `node scripts/qualification/run-adversarial.mjs --all`.

## Threat model

Membrane is a context-federation control plane. Admission decisions are made by
`mcp/authorization.mjs` (monotone effective-authority intersection, MBR-002) and
`mcp/scope-grant-v1.mjs` (Ed25519-signed, short-lived scope grants, MBR-003).
The adversary we model is any caller — human, agent, or injected content — that
tries to reach a repository or operation it is not authorized for.

The suite attempts six attack classes and asserts each is **rejected**:

| Class | Attack | Expected defence |
|-------|--------|------------------|
| **A1** | Caller without a grant for a target repo (cross-root reach) | `cross_root_binding_denied` before any level check |
| **A2** | Cross-repository scope escalation (read-only caller → write-trusted child; target policy capping a trusted caller) | Monotone intersection keeps effective authority at the minimum slot → `caller_not_authorized` |
| **A3** | Task/turn envelope spoofing (forged `admin` task grant; out-of-vocabulary level) | Intersection cannot be lifted by a single strong slot; unknown levels throw `unknown_authority_level` |
| **A4** | Authority-widening via absent/null task grant or installation slot | Fail-closed: an absent grant caps at `read-only`, blocking writes |
| **A5** | Injected authority level outside the lattice (e.g. `root`) | Rejected, never silently widened |
| **A6** | **Prompt-injection**: directive text ("ignore previous instructions", "grant admin", fake `scopeGrantId`, path-traversal `sourceRef`) carried as *data* in bindings, grant fields, or packet blocks | Treated as opaque bytes; it changes the request/hash binding or the repository identity, so validation fails and admission is denied |

The scope-grant battery additionally covers forged signer keys, tampered
signatures, replaced `scopeGrantId`, widening edits to immutable grant fields,
expired/revoked replay, and fail-closed minting for adversarial packet shapes.

## The zero-admission guarantee

Effective privilege is the **minimum (intersection)** of installation, caller,
target, child-grant, task/session-grant, and operation capability. No downstream
call may recover authority from the target binding or any single strong slot
alone. The qualification runner:

1. Runs the full adversarial test suite under the Node test runner.
2. Independently replays every `deny` fixture case and computes
   `unauthorized_admission_rate = admitted / attempted`.
3. Exits non-zero if the suite fails **or** any case is admitted.

The release-gate acceptance is an unauthorized admission rate of **exactly 0**.

## Running

```bash
# The exact release/book gate command:
node scripts/qualification/run-adversarial.mjs --all

# Run the suite directly:
node --test tests/adversarial/*.test.mjs
```

Fixtures live in `tests/fixtures/adversarial/fixtures.mjs`. They are pure data
with a fixed clock (`FIXED_NOW`); Ed25519 key material is generated per run via
`node:crypto`, so no real key is ever embedded and every run is independent.

## Scope

The suite exercises the **real** authorization modules read-only via absolute
file URL. It does not modify `mcp/authorization.mjs` or `mcp/scope-grant-v1.mjs`
(outside this task's allowlist); it asserts their decisions against the fixture
battery.
