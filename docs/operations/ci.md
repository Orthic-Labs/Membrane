# Continuous integration

Cortex runs three workflows on pull requests and pushes to `main`:

## `ci` — cross-platform tests

Matrix: Ubuntu, macOS, Windows × Node 22.13.1, 24.x. Runs:

1. `pnpm test` (fast Node suite)
2. `pnpm test:random` (randomized test order)
3. `pnpm test:all` (serialized full suite)
4. `node scripts/ci/check-generated.mjs` (generated-doc drift)
5. `node scripts/ci/check-network-boundary.mjs` (network inventory)

## `qualification` — schema/security lanes

Runs on graph/schema/tests/evals changes:

- `qualification` — retrieval benchmark contract + full suite
- `schema` — contract-catalog tests + grammar inventory
- `security` — network-boundary inventory

## `package` — clean tarball

Runs `node scripts/test-package.mjs` on all three OSes: dry-run allowlist,
real tarball extraction, production dependency install, help, and MCP
handshake outside the monorepo.

## Required branch-protection checks

Gate `main` on the exact job names:

- `ci / test (ubuntu-latest, 22.13.1)`
- `ci / test (ubuntu-latest, 24.x)`
- `ci / test (macos-latest, 22.13.1)`
- `ci / test (macos-latest, 24.x)`
- `ci / test (windows-latest, 22.13.1)`
- `ci / test (windows-latest, 24.x)`
- `qualification / qualification`
- `qualification / schema`
- `qualification / security`
- `package / clean-tarball (ubuntu-latest)`
- `package / clean-tarball (macos-latest)`
- `package / clean-tarball (windows-latest)`

No workflow may publish. Release publishing happens only in the
release-candidate / release workflows behind protected environments.
