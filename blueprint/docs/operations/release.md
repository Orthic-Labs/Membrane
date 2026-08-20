# Release process

## Release chain

1. Commit and push the exact qualified source tree. RightGit CI must pass on
   that commit; CI never receives signing material.
2. From the clean primary checkout, run RightRelease locally with explicit
   `patch` lane. Build, signing, notarization, hardening, and sealing finish
   before any upload.
3. Exercise sealed macOS and Windows installers on clean hosts through
   install → init → query → MCP → update → rollback → uninstall. Missing or
   failing receipts stop publication.
4. Generate compatibility, catalog, checksums, SBOM, provenance, and signed
   updater metadata from sealed bytes under ignored `.right-release/` output.
   Generate channel manifests from immutable GitHub Release assets.
5. Publish and redownload-verify immutable bytes in order: GitHub Release,
   npm OIDC, MCP Registry, Orthic Homebrew tap, Scoop, then WinGet.

Apple and Azure signing are provisioned workspace capabilities owned by
RightRelease. Product workflows do not implement signing or carry secrets.
RightGit owns public CI and npm OIDC publication.

## Candidates (non-publishable)

`node scripts/release/build-candidate.mjs --platform current --out <dir>`
builds an unsigned release candidate:

- `compatibility.json` — product/version/commit, platform/arch, Node version,
  store/schema versions, grammar manifest digest, per-file SHA-256, signed:false
- `checksums.txt` — SHA-256 for every artifact
- `SBOM.spdx.json` — SPDX-2.3 JSON
- `THIRD_PARTY_NOTICES` — dependency license notices
- `artifact-catalog.json` — machine-readable catalog
- `update-manifest.json` — rehearsal `UpdateManifestV1`, marked
  `publishable:false`, ready for local signing

The builder rejects dirty trees, mismatched versions, missing notices, and
non-allowlisted files. No workflow may publish.

Verify with `node scripts/release/check-release.mjs <dir>` — it re-checksums
every artifact, cross-checks `checksums.txt`, and validates the SBOM and
catalog plus the update manifest against the candidate identity and
artifacts.

## Signing and publishing

RightRelease owns signed macOS/Windows artifacts. RightGit owns public CI and
npm OIDC. `release-candidate.yml` may assemble non-publishable candidates but
never signs or publishes.

## Release seal (D53 / EC v4 D-18)

The sealed release gate chain (D53) no longer cites the deleted
in-repo signing workflow (removed at `ec7253d`) and its jobs
(`macos-sign-and-notarize`, `windows-sign`) which violate
the signing doctrine fixed by CU-17/CU-18. Per D-18:

- **CU-13** satisfies SBOM / checksums / provenance via
  `scripts/release/check-release.mjs`, `checksums.mjs`, `sbom.mjs` (present at
  HEAD; see `scripts/release/verify-release.mjs` check).
- **CU-17 / CU-18** satisfy macOS / Windows signing via `right-release`
  invoked from the primary checkout — no in-repo signing workflow, no
  Apple/Azure secrets in `.github/workflows/*.yml`.
- **Clean-host verification** is the `clean-host-smoke` matrix already
  present in CU-17/CU-18 (`PKG install → {blueprint,watch,MCP,update,uninstall}`
  on macOS; `%LOCALAPPDATA%\Orthic\Blueprint` round-trip on Windows).

Orchestration lives in `.github/workflows/release-candidate.yml` (extended
by v4-U53, not a resurrected in-repo signing workflow): it runs
`pnpm test:all`, contracts, security/hardening, `check-network-boundary`,
`test-package`, `build-candidate → check-release` (CU-13 outputs), the
CU-14 runtime bundle test, `stage-runtime` / `build-runtime-bundle`,
the Hub installer byte-identical attach (v4-U60, below), and
`scripts/release/verify-release.mjs` as the gate that asserts no
deleted workflow is cited and no in-repo signing job exists.

Evidence is `.agent/dispatch/state.json` D53 entry:
`release seal via .github/workflows/release-candidate.yml orchestrating
CU-13 + CU-17/CU-18 + clean-host install verification; no in-repo signing
workflow per EC v4 D-18`.

Verify locally:

```
node scripts/release/verify-release.mjs <candidate-dir>
# Check no deleted workflow is cited outside historical contract docs:
grep -rn "immutable-release" .agent .github docs --include="*.json" --include="*.yml" --include="*.md" | grep -v "docs/plans"   # → 0
```

## Hub installer — byte-identical attachment (v4-U60 / D-S07)

Blueprint does not build the Hub app. The Hub's installer is built once in
`orthic-hub` and attached byte-identical to blueprint's own GitHub Release:

- Workflow step in `release-candidate.yml`:
  `node scripts/release/verify-hub-installer.mjs --hub-version "${{ vars.HUB_APP_VERSION }}" --checksum "${{ vars.HUB_INSTALLER_CHECKSUM }}" --out "${{ runner.temp }}/hub-installer"`
  downloads the pinned Hub artifact
  (`https://github.com/Orthic-Labs/orthic-hub/releases/download/v${HUB_APP_VERSION}/Orthic-${HUB_APP_VERSION}.dmg`),
  verifies `sha256`, and attaches the file. Mismatched checksums throw
  `checksum_mismatch` and fail the job — never a blueprint-side rebuild.
- Catalog pin: `release/catalog.template.json` carries `hubAppVersion` and
  `hubInstallerChecksum` (`__HUB_APP_VERSION__` / `__HUB_INSTALLER_CHECKSUM__`
  placeholders, filled by `check-release.mjs` / `verify-hub-installer.mjs`).
- Local dry-run (mocked fetcher used in tests):

```
node scripts/release/verify-hub-installer.mjs --hub-version 0.1.0-test \
  --checksum <sha256> --out /tmp/hub-installer
node --test tests/release-candidate.test.mjs   # U60 mismatch-rejection case
```

This unit does not fix D53 by itself — v4-U53 orchestrates the seal
around it; U60 supplies only the Hub-installer half of "release".
