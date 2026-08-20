# Updates

`cortex update` is channel-aware and signed:

```sh
cortex update check    --channel stable|beta|nightly [--offline] [--json]
cortex update apply    --channel stable|beta|nightly [--offline] [--json]
cortex update rollback [--json]
```

## Install-owner detection

| Owner | Behavior |
|---|---|
| npm / pnpm | Prints `npm update -g <pkg>`; never self-replaces. |
| Homebrew | Prints `brew upgrade cortex`; never self-replaces. |
| WinGet | Prints `winget upgrade OrthicLabs.Cortex`; never self-replaces. |
| Portable / native | Requires a signed release manifest and matching checksum before staging. |

## Safety rules

- No unsigned self-update is possible.
- The database is backed up before any incompatible migration.
- App replacement is atomic; one prior version and a compatible store backup
  are retained.
- Downgrades and replay manifests are rejected.
- Rollback restores the prior app version and the store backup.

## Offline and opt-out

- `--offline` disables update checks.
- `CORTEX_NO_UPDATE_CHECK=1` disables update checks entirely.
- Update checks never run during ordinary indexing or querying.

## Network endpoints

Every network endpoint used by updates is inventoried in
`scripts/ci/check-network-boundary.mjs`.
