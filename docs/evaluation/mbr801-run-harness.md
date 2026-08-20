# MBR-801 installed-path ten-scenario harness

`scripts/qualification/run.mjs` is the "exact installed-path ten-scenario
harness" the Book 2 gate invokes as:

```bash
node scripts/qualification/run.mjs --task MBR-801 --platform macos --release-manifest <path/to/release-evidence.json> --event-db <path>
```

## What it exercises

For the running platform it:

1. Proves the build under test is a **signed, installed** build by
   delegating to `scripts/release/verify-release-evidence.mjs` against a
   real release-evidence manifest (artifact hash, ed25519 signature,
   Apple notarization platform trust, installed platform
   receipts).
2. Resolves the **real client, model, and host** identity running the
   harness (`CORTEX_CLIENT`, `MEMBRANE_QUALIFICATION_MODEL`, machine
   hostname).
3. Runs all ten canonical scenarios (`repository_orientation`,
   `cross_repo_impact`, `preference_application`,
   `stale_graph_immediate_edit`, `contradiction`, `denied_scope`,
   `tool_proof_criteria`, `user_correction`, `memory_temporal_as_of`,
   `provider_timeout_degradation`) against the real, already-installed host
   by delegating to `scripts/run-platform-scenarios.mjs`, which drives a real
   client CLI, real providers, real delivery, and a real outcome/feedback
   write against the live event-log database.
4. Aggregates a real benchmark over the resulting traces via
   `mcp/e2e-benchmark.mjs`.
5. Archives a per-platform `receipt.json` plus one archive file per scenario
   trace, in the exact shape `scripts/qualification/verify-mbr801-evidence.mjs`
   requires: signed/installed proof, real client/model/host identity, all ten
   scenarios with unique trace IDs and passed provider/delivery/outcome
   gates, and a complete benchmark.

The receipt's `status` is `"passed"` only when every one of those checks
holds; otherwise it is `"incomplete"` with a `reasons` array naming exactly
what failed. Nothing is ever marked passed by omission.

## Injectable dependencies (why this is testable without a live install)

Every real-execution dependency — signed-build verification, host-identity
resolution, scenario execution, and benchmark aggregation — is an injectable
parameter of `runInstalledPathHarness(options)`, each defaulting to the real,
installed-path implementation described above. `tests/e2e/mbr801-run-harness.test.mjs`
injects deterministic fakes for a `macos` platform run and
proves:

- all ten scenarios complete with unique trace IDs,
- the resulting receipts, fed straight into the existing
  `verifyMbr801Evidence` validator, verify as `"passed"` under one current
  commit and release generation,
- a missing trace, a duplicate trace, an unsigned build, or an incomplete
  benchmark each fails the receipt closed with a specific reason.

## Manual execution preconditions

The default (real) implementations require, on the machine running the
harness:

- a signed release-evidence manifest (`--release-manifest`) produced by the
  real signed release pipeline,
- a running installed Membrane service and the installed `cortex` CLI,
- a live event-log database (`--event-db`),
- a real installed client host reachable the way
  `scripts/run-platform-scenarios.mjs` expects.

These preconditions are why this harness is invoked manually at the Book
gate on a user-controlled macOS machine, never during task
implementation and never from an automated pipeline.
