# macOS signing

`scripts/release/macos/sign-and-package.sh` signs and notarizes macOS release
artifacts. It is owner-credential-gated: it refuses to run without the exact
S-14 protected values.

## Required protected values

```text
APPLE_TEAM_ID
APPLE_DEVELOPER_ID_APPLICATION
APPLE_DEVELOPER_ID_INSTALLER
APPLE_NOTARY_KEY_ID
APPLE_NOTARY_ISSUER_ID
APPLE_NOTARY_KEY_P8_BASE64
```

Never print certificate or API-key material.

## Order

1. Sign every Mach-O in the staged bundle (`codesign --options runtime
   --timestamp`).
2. Verify (`codesign --verify --deep --strict`).
3. Build and sign a per-user PKG (`pkgbuild` + `productbuild`).
4. Submit with `notarytool`, staple, verify with `spctl` and `pkgutil`.

## Verify only

```sh
bash scripts/release/macos/sign-and-package.sh <path> --verify-only
```

## Packaging rules

- Package install must not enroll repositories or install host hooks without
  a later explicit `cortex init`.
- The optional DMG is a presentation wrapper containing the notarized PKG,
  release notes, and uninstall script.
- Signed artifacts upload only after clean-host PKG install, service, CLI,
  MCP, update, and uninstall tests pass.
- Uninstall is complete and data-preserving by default.

# Windows signing

`scripts/release/windows/` contains the per-user Inno Setup installer,
Azure Artifact Signing integration, and signature verification.

## Required protected values

```text
AZURE_CLIENT_ID
AZURE_TENANT_ID
AZURE_SUBSCRIPTION_ID
AZURE_ARTIFACT_SIGNING_ENDPOINT
AZURE_ARTIFACT_SIGNING_ACCOUNT
AZURE_ARTIFACT_SIGNING_PROFILE
```

## Order

1. `azure/login@v3` + `azure/artifact-signing-action@v2` on a Windows runner.
2. Sign `.exe`, `.dll`, `.msi`/installer payloads recursively with SHA-256 and
   RFC3161 timestamping.
3. Verify every signable file after signing.
4. Install and uninstall in a clean non-admin profile; assert PATH, scheduled
   task, files, logs, and registry entries are correct and removable.

Windows arm64 is not claimed until a separate target matrix passes.
