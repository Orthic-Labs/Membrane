# Membrane Session B — initial independent dogfood report

## Verdict

**BLOCKED — not fully verified.** The frozen contract has 17 required product cases. Zero product cases executed, zero passed, zero failed semantically, and all 17 are blocked by absent repository dependencies and/or the absence of a revision-compatible native/installed artifact. The independent oracle's negative-control harness passed, but that is deliberately excluded from product coverage.

This is not an all-green result and not evidence that intended product journeys are ready. It also is not a source defect verdict: the attempted public server never loaded far enough to observe Membrane behavior.

## Target and independence

The target is `a4e0a9e02c64e5439cdec289a160b748f5e5686c`, selected from current `HEAD` because Adrian explicitly said to ignore the packet's embedded baseline. The checkout exposes neither a configured Git remote nor a `main` ref, so remote-main reachability and intervening revisions remain unknown. Phase-one hashes were frozen before implementation/body inspection or execution. No Session A artifact, report, test, message, finding, or conclusion was read or requested.

## What was produced

* An immutable versioned oracle with 17 product cases across every required journey and explicit allowed/forbidden evidence.
* Two repository fixtures, two distinct identical document identities, and isolated admitted/conflicting/rejected knowledge truth, all independently hashed.
* A thin stdio public-boundary test that performs real MCP discovery/call when dependencies exist; it does not replace planner, dispatch, authorization, or publication logic.
* Seven harness negative controls. They reject identity mismatch, stale digest, absent facts, duplicate bodies, omitted degradation, and fake-clean diagnostics.
* A no-build runbook and machine-readable results with exact evidence-layer classifications.

## Attempted execution

`node --test audit/session-b/tests/public-boundary-dogfood.test.mjs` ran three harness/test cases. `ORACLE-NEG` passed. `A01` and `B01` were classified BLOCKED because the public server process failed at module loading: `@modelcontextprotocol/server` is absent. This is an environment prerequisite, not a Membrane behavioral observation.

The normal pinned package manager is also unavailable in this environment: pnpm 11.24.0 reports that it requires Node >=22.13 and then fails because Node v20.20.2 lacks `node:sqlite`. The workspace forbids ad hoc toolchain installation. No Cargo, build, product test, package, install, workflow, runtime activation, or user-data command was run.

## Coverage and unresolved prerequisites

| Layer | Result | Precise prerequisite |
|---|---|---|
| Oracle self-check | PASS | none |
| Retained JavaScript MCP | BLOCKED | normal workspace bootstrap with pinned Node and installed locked dependencies |
| Native MCP/Hub/Blueprint/Ledger/Cortex/Adapt/Push | BLOCKED | platform-compatible prebuilt artifact with receipt binding exact target SHA, isolated data roots, tray-owned supported launch |
| Simulated host final envelope | BLOCKED | live exact native path plus negotiated resolver capability |
| Actual installed adapters/platform | BLOCKED | isolated installer-owned current root and host qualification; active user installation remains untouched |

The next valid run should satisfy these prerequisites through the existing workspace/CI distribution path, then execute the runbook without modifying the frozen oracle. Mandatory native/installed blockage continues to prevent any blanket product-verification claim.

## Files changed

Only `audit/session-b/**` test/report artifacts were added in the isolated detached worktree. Production code, production tests, canons, allowlists, flags, configuration, release state, and the shared checkout/index were not changed.

## Repair: complete runnable blocked-case suite

Following independent completion validation, the initial report and results were preserved byte-for-byte as `INITIAL-DOGFOOD-REPORT.md` and `INITIAL-DOGFOOD-RESULTS.json`, with hashes in `INITIAL-RESULTS-SHA256SUMS`. The frozen oracle and charter remain unchanged.

`tests/native-public-boundary-dogfood.test.mjs` now supplies executable public-MCP cases for every previously missing oracle ID: B02, C01–C02, D01–D02, E01, F01, G01, H01, I01, J01, K01, L01–L02, and R01. These tests invoke the exact prebuilt binary over `stdio-mcp`, use task-owned mutable fixture copies, and assert independently authored facts, forbidden evidence, identities, bounds, or typed outcomes. They skip only when the exact native/Hub/installed prerequisite is absent.

The repaired suite collected all 15 added cases successfully in this cloud environment; all 15 reported precise prerequisite-based skips. This improves runnable coverage but does not change the empirical product verdict: the artifact-dependent cases remain BLOCKED, not PASS.
