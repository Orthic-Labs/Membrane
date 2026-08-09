# Release process

## Release chain

1. **Rehearsal (signed native).** With signing credentials present, run
   the immutable release chain as a dry run (publish/attest disabled). The
   signed macOS and Windows installers are exercised on clean hosts through
   install → init → query → MCP → update → rollback → uninstall, and each
   host uploads a signed-native receipt (`cortex-macos-clean-host-receipt`
   and `cortex-windows-clean-host-receipt`) as an artifact of that run.
   Record the dry-run run ID.
2. **Receipt artifacts.** Both receipts must exist as artifacts of the
   rehearsal run. A missing download fails the release gate; no receipt is
   ever defaulted to passing.
3. **`release.yml` with `rehearsal_run_id`.** Dispatch the release
   workflow, passing the rehearsal run ID. It calls `immutable-release.yml`
   with `dry_run: false`.
4. **Immutable verification.** The native-clean-host gate downloads both
   receipts from the rehearsal run and verifies each against
   `release/catalog.json` and the requested version. The update manifest is
   signed with the owner-held `UPDATE_SIGNING_KEY_PEM` secret when
   configured; absent the secret the signing step is skipped and the
   shipped `update-manifest.json` stays unsigned.
5. **Provenance.** Attestations (SLSA build provenance + SBOM) are issued
   only after the receipt gate passes.
6. **Publish.** `publish-npm` runs only after clean-host install proof and
   provenance, publishing the candidate-bound tarball.

The following steps remain owner-credential-gated and cannot run until
Adrian supplies them:

- Apple Developer ID application + installer certificates, notary key, and
  team ID (`APPLE_*` secrets) — needed for the signed-native rehearsal and
  for `macos-sign-and-notarize`.
- Azure code-signing credentials (`AZURE_*` secrets) — needed for
  `windows-sign`.
- `UPDATE_SIGNING_KEY_PEM` — the Ed25519 private key that signs the update
  manifest; without it the signing step is skipped and the shipped update
  manifest stays unsigned.
- `WINGET_CREATE_GITHUB_TOKEN` — classic PAT with `public_repo` scope for
  the WinGet submission lane in `publish-package-managers.yml`.

## Candidates (unsigned)

`node scripts/release/build-candidate.mjs --platform current --out <dir>`
builds an unsigned release candidate:

- `compatibility.json` — product/version/commit, platform/arch, Node version,
  store/schema versions, grammar manifest digest, per-file SHA-256, signed:false
- `checksums.txt` — SHA-256 for every artifact
- `SBOM.spdx.json` — SPDX-2.3 JSON
- `THIRD_PARTY_NOTICES` — dependency license notices
- `artifact-catalog.json` — machine-readable catalog
- `update-manifest.json` — unsigned `UpdateManifestV1` (schema-valid
  placeholder `keyId`/`signature`) ready for the signing step

The builder rejects dirty trees, mismatched versions, missing notices, and
non-allowlisted files. No workflow may publish.

Verify with `node scripts/release/check-release.mjs <dir>` — it re-checksums
every artifact, cross-checks `checksums.txt`, and validates the SBOM and
catalog plus the update manifest against the candidate identity and
artifacts.

## Signing and publishing

Signed macOS/Windows artifacts (D17/D18) and package-manager publication
(D19) run only behind owner credential gates and protected environments.
`release-candidate.yml` never signs or publishes.
