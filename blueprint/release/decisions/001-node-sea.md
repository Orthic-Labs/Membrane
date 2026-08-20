# Decision: Node SEA (Single Executable Application)

**Status:** Deferred
**Owner:** Orthic Labs maintainers
**Date:** 2026-08-06
**Packet:** D53

## Decision

Node Single Executable Application packaging is **deferred** for the 1.0
release. The 1.0 release uses the portable runtime bundle shipped by
`scripts/release/stage-runtime.mjs` plus signed native `.pkg` installer.

## What was measured

- A Node SEA build would let us ship a single executable with no
  `lib/node/` directory. Measured 2026-08-04: a SEA build of the blueprint
  runtime peaks at ~110 MB on a fresh machine and the postinstall /
  uninstall flow becomes platform-specific.
- The current portable runtime bundle (D14) is already zero-prerequisite
  on macOS; future non-Mac targets require separate qualification.
  All three platforms already meet the runbook's "no system Node required"
  contract.

## Why deferred, not declined

- A SEA build would be useful for sandboxed environments that disallow
  bundling a Node runtime (e.g. very locked-down base images).
- A SEA build is also a prerequisite for moving the resident service to a
  per-user launchd executable that does not depend on the
  per-installation runtime bundle.
- Neither is in scope for 1.0; both are recorded as a future option.

## Reversal conditions

- A user-facing feature requires a single-executable distribution.
- A packaging regression makes the runtime bundle impractical on a
  supported platform.
