# Windows release contract

MBR-902 ships `Membrane_<semver>_x64-setup.exe` (Tauri NSIS). Build and signing are owned by RightKit, not this repository. On the Windows machine, from the primary checkout, after pulling this commit:

```
pnpm --dir apps/membrane-hub release:doctor -- --platform win   # right-release doctor
pnpm --dir apps/membrane-hub release:build:win                  # right-release build --platform win
```

`release:build:win` runs `rightkit:package:win` (`tauri build --bundles nsis`), then signs `targets.win.sign.files` from `apps/membrane-hub/right-release.config.mjs` with Azure Artifact Signing (SHA-256 digest, RFC3161 timestamp, `signtool /dlib`) via `tools/rightkit/packages/release/sign-windows.mjs`, then runs RightKit's `hardeningscan.mjs`, then seals a `release-manifest.json` (schema `1`). Nothing in this repository invokes AzureSignTool, `signtool`, `cargo`, `pnpm build`, or Tauri directly -- earlier versions of this contract did, duplicating RightKit, and that logic has been removed.

`scripts/release/windows/contract.mjs`'s `validateSealedManifest` reads that RightKit-produced `release-manifest.json` directly -- never a bespoke schema -- and fails closed unless it is a `schema: 1`, `platform: "win"` manifest whose `checkpoints` include `signed`, `hardened`, and `sealed`, and whose `files[]` names a `Membrane_<version>_x64-setup.exe` installer entry with a SHA-256 hash.

## The inner-PE gap RightKit does not close today

RightKit's `sign.files` and `installer.artifacts` in `apps/membrane-hub/right-release.config.mjs` currently name only the outer NSIS installer. RightKit signs `sign.files` strictly *after* `target.package` (`tauri build --bundles nsis`) runs -- after NSIS has already embedded the main app binary and the two `bundle.externalBin` sidecars (`crypt-service`, `membrane`; see `src-tauri/tauri.conf.json`) inside the installer, via `src-tauri/windows/installer.nsi`'s "Copy main executable" and "Copy external binaries" sections. Signing a file on disk after it has already been copied into the installer does not sign the copy already inside the installer.

So today, RightKit's sealed manifest proves the outer installer is signed; it proves nothing about the three inner PEs (`membrane-hub.exe`, and the two sidecars) NSIS embeds inside it.

This is a real, confirmed gap, and it cannot be closed from `scripts/release/windows/**` alone: closing it means editing `apps/membrane-hub/right-release.config.mjs` and probably `apps/membrane-hub/package.json`'s `rightkit:package:win` script so the compiled binaries are signed *before* NSIS bundles them -- both files are outside this repair's allowed scope. What this repair does instead is require and fail closed on explicit, out-of-band proof:

- `minimumExpectedPayloadCount` reads `apps/membrane-hub/src-tauri/tauri.conf.json`'s `bundle.externalBin` (read-only; this repair never edits `apps/membrane-hub/**`) to compute the minimum inner-PE count -- 2 sidecars + 1 main binary = 3 today.
- `validateLifecycleReceipt` fails closed unless a `signtool verify /pa /tw` proof -- `{ name, sha256, verify: "pass" }` -- is recorded for every one of those payloads, in addition to the outer installer's signature and the clean-machine install/update/uninstall gates, which RightKit does not run either.

## Verification

```
node scripts/release/windows/verify-release.mjs <release-manifest.json> <expected-payloads.json> <receipt.json> [tauri.conf.json]
```

validates all three together: RightKit's sealed manifest, the expected inner-PE list (checked against the Tauri config's sidecar count when a `tauri.conf.json` path is given), and the lifecycle receipt proving `signtool verify /pa /tw` for the installer and every inner PE plus a clean install/update/uninstall pass. `packaging/windows/release-contract.json` is the machine-readable version of this same contract. Missing or mismatched inputs fail closed. A passing contract is not artifact acceptance: signed installer bytes and clean Windows receipts, produced on the Windows machine, remain required.

Azure Artifact Signing authenticates on the Windows signing machine itself and may require a PIN or passkey; that step stays with Adrian, on Windows, run through RightKit -- nothing here prepares, prompts for, or routes around it.

## Compatibility note

`tests/release/windows.test.mjs` (pre-existing, outside this repair's allowed file set) still imports and exercises `validateReleaseManifest` / `signingPlan` / `validateReceipt` from `contract.mjs`. Those three exports are kept byte-for-byte as a compatibility shim so that test keeps passing, but they are no longer wired into any script here: `signingPlan`'s AzureSignTool/SignTool argv construction duplicated RightKit's own `tools/rightkit/packages/release/sign-windows.mjs`, and nothing calls it to actually sign anything anymore. `evidence/platform/windows/windows-release-manifest.json` (also outside this repair's allowed scope) is a hand-authored fixture in the same superseded schema, and only lists one of the three inner PEs; it should be reconciled or removed by someone with access to `evidence/platform/windows/**`.
