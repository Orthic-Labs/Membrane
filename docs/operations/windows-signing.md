# Windows signing (D18)

Ground truth: `.github/workflows/immutable-release.yml` (`windows-sign` job,
~lines 156-194), `scripts/release/windows/build-installer.ps1`,
`scripts/release/windows/verify-signatures.ps1`, `release/windows/Cortex.iss`.

## Signing pipeline

The `windows-sign` job runs on `windows-latest` under the `release`
environment with `id-token: write`, and requires `dry_run != 'true'`:

1. Stage the runtime bundle with `scripts/release/stage-runtime.mjs`.
2. Authenticate via **Azure OIDC** — `azure/login@v3` using
   `AZURE_CLIENT_ID`, `AZURE_TENANT_ID`, `AZURE_SUBSCRIPTION_ID` secrets.
   No client secret is used or stored.
3. Sign with **Azure Artifact Signing** — `azure/artifact-signing-action@v2`,
   using `AZURE_ARTIFACT_SIGNING_ENDPOINT` / `_ACCOUNT` / `_PROFILE` from
   repository **vars** (not secrets). Signs `.exe` and `.dll` files
   recursively (`files-folder-recurse: true`) with `file-digest: SHA256`
   and an RFC3161 timestamp (`timestamp-rfc3161:
   http://timestamp.acs.microsoft.com`, `timestamp-digest: SHA256`).
4. Build the installer with
   `scripts/release/windows/build-installer.ps1`, which compiles
   `release/windows/Cortex.iss` via Inno Setup (ISCC).
5. Verify every signable payload with
   `scripts/release/windows/verify-signatures.ps1` — asserts
   `Get-AuthenticodeSignature` status is `Valid` and the algorithm is
   SHA-256 for every `.exe`/`.dll`/`.msi` under the staged root; a single
   unsigned or wrong-algorithm file fails the job.
6. Upload the signed installer as the `cortex-windows-signed` artifact.

## Installer identity

`release/windows/Cortex.iss` builds a **per-user** installer:
`DefaultDirName={localappdata}\Orthic\Cortex` (i.e.
`%LOCALAPPDATA%\Orthic\Cortex`), `PrivilegesRequired=lowest` — no admin
rights required to install.

## Owner gate

`native-clean-host-owner-gate` (needs `macos-sign-and-notarize` and
`windows-sign`) fails by design until a real signed, native, clean-host
install receipt exists for D17/D18: it runs
`node -e "console.error('native_clean_host_owner_gate_missing_D17_D18_receipt'); process.exit(1)"`.
Per `.agent/dispatch/state.json`, D17/D18 evidence is recorded as
"LIVE SIGNING OWNER-GATED (not-run)" — the workflow scaffolding, signing
steps, and verification are implemented and pass CI, but no real Azure
signing identity/secrets have been provisioned, so live signing and the
clean-host receipt gate block every real release until an owner supplies
them.

## Related

- `docs/operations/uninstall.md` — Windows uninstall paths
  (`release/windows/uninstall-check.ps1`).
