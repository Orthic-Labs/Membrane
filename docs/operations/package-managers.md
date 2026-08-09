# Package managers

Cortex publishes through several channels, all converging on the same
product and exact checksums from `release/catalog.json`.

## Identities

| Channel | Identity |
|---|---|
| npm | `@orthic-labs/cortex` |
| Homebrew tap | `Orthic-Labs/homebrew-tap`, formula `cortex` |
| WinGet | `OrthicLabs.Cortex` |
| Scoop | `cortex` |
| MCP registry | `io.github.Orthic-Labs/cortex` |
| Service ID | `io.orthic.cortex` |
| Container | `ghcr.io/orthic-labs/cortex` |

## Rules

- Every URL references an immutable release asset (exact version), never
  `latest`.
- Every manifest hash is copied from `release/catalog.json` by script, never
  by hand.
- Homebrew installs the same portable archive plus completions/man page.
- WinGet uses the signed per-user installer.
- Docker is documented for CI/headless use only.
- MCP registry metadata launches `cortex mcp serve` from the published npm
  package.
- Publication runs only after release verification in protected
  environments; it may open downstream PRs but never rewrites an existing
  release.

## Validation

```sh
node --test tests/package-manager-manifests.test.mjs
ruby -c release/homebrew/cortex.rb
node -e "JSON.parse(require('fs').readFileSync('server.json','utf8'))"
```

## WinGet submission

`publish-package-managers.yml` runs a Windows `winget` job behind the
`release` environment. It is fail-closed: without the
`WINGET_CREATE_GITHUB_TOKEN` secret the job is skipped entirely.

- `submit=false` (default): `wingetcreate update` runs output-only into a
  temp directory and every generated `OrthicLabs.Cortex.*` manifest is
  `wingetcreate validate`d. Nothing is pushed. Release-triggered runs
  always take this path.
- `submit=true` (workflow_dispatch only): the same update runs with
  `--submit` and opens a PR against `microsoft/winget-pkgs`.
- The package version and installer URL are derived from
  `release/catalog.json` at run time, never hardcoded in the workflow.
- The token is never written into the workflow; it reaches wingetcreate
  only through the `WINGET_CREATE_GITHUB_TOKEN` environment variable.

The submit path requires a classic PAT with the `public_repo` scope so the
PR against `microsoft/winget-pkgs` can be opened. **Adrian must create
it** and store it as the `WINGET_CREATE_GITHUB_TOKEN` repository secret.
