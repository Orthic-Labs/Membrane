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
