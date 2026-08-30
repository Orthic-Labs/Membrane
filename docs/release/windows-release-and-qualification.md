# Windows release and installed-artifact qualification

## Tag path

The workflow is `.github/workflows/release-candidate.yml`. The exact tag command is:

```text
git tag v<version>
git push --tags
```

The ordered job graph for a `v*` tag is:

```text
private-repo-guard
  -> candidate: build -> check -> upload explicitly UNSIGNED candidate
  -> finalization: download candidate -> sign/package with RightKit -> bind qualification manifest/SBOM
  -> installed-qualification: download same-run signed inputs -> install and qualify
  -> publish: publish the RightKit portable release/bootstrap -> attach installer, qualification manifest, SBOM, and evidence
```

`candidate` is public CI. It runs the existing candidate build and check and writes
`UNSIGNED-CANDIDATE.txt`. That artifact is never presented as signed and is not a customer
artifact.

`finalization` runs on the runner named by `vars.MEMBRANE_QUALIFICATION_RUNNER_LABEL`. It downloads
the candidate from the same workflow run, runs the existing RightKit Windows build/signing path,
runs the existing portable finalization path, and emits the signed installer, the RightKit
portable-release inputs, and a qualification-bound `release-manifest.json` and `sbom.json`.
The qualification manifest and SBOM bind the installer SHA-256.

`installed-qualification` runs on the same configured self-hosted Windows runner. On a tag it
uses `actions/download-artifact` to obtain those finalization outputs from the same run. It obtains
the newest prior non-draft release's installer for the downgrade leg, then runs
`scripts/qualification/install-release.ps1`. Missing signing, runner, prior-release, installer,
manifest, or SBOM prerequisites fail the tag path with the typed
`installed_qualification_skipped { reason: prerequisites_absent, detail: ... }` reason. The job
never converts a hosted runner or an unsigned artifact into a pass.

The script exercises the installed installer through install, startup, tray/UI automation,
Blueprint checks, downgrade, upgrade, state continuity, uninstall, and residue checks. The
product under test is the installed artifact. The development workspace is for development and
testing only; it is never the thing a customer runs.

`publish` runs only after `installed-qualification` succeeds. It has the only job-level
`contents: write` permission. RightKit publishes the signed portable release and stable bootstrap
through its existing GitHub and R2 publication path; the workflow then attaches the signed
installer, qualification manifest, SBOM, and qualification evidence to the GitHub Release. A
failed qualification cannot reach publication.

## Required configuration

### Tag path

| Kind | Exact name | What it enables | What breaks without it |
|---|---|---|---|
| Repository variable | `MEMBRANE_QUALIFICATION_RUNNER_LABEL` | Protected finalization, installed qualification, and publication on a self-hosted Windows runner with an interactive desktop | Finalization and qualification fail with `qualification_runner_missing`; tray UI assertions cannot be moved to a hosted runner |
| Protected-host environment | `AZURE_ARTIFACT_SIGNING_METADATA` or `AZURE_SIGNING_METADATA` | A provisioned RightKit Azure signing metadata file | Signing finalization fails with `signing_configuration_missing` unless the three variables below are present |
| Protected-host environment | `AZURE_ARTIFACT_SIGNING_ENDPOINT` | Azure Artifact Signing endpoint when metadata is assembled by RightKit | Signing finalization fails with `signing_configuration_missing` |
| Protected-host environment | `AZURE_ARTIFACT_SIGNING_ACCOUNT` | Azure Artifact Signing account when metadata is assembled by RightKit | Signing finalization fails with `signing_configuration_missing` |
| Protected-host environment | `AZURE_ARTIFACT_SIGNING_PROFILE` | Azure Artifact Signing certificate profile when metadata is assembled by RightKit | Signing finalization fails with `signing_configuration_missing` |
| GitHub Actions secret | `CLOUDFLARE_API_TOKEN` | Stable bootstrap publication through the public R2 bucket | Publication fails with `bootstrap_publication_missing` before any release mutation |

The protected-host Azure values are consumed from the provisioned host environment; they are not
placed in the workflow as secret values. RightKit also requires its installed Windows signing
client and `signtool.exe`; `AZURE_CODESIGN_DLIB_PATH`/`AZURE_ARTIFACT_SIGNING_DLIB_PATH` and
`AZURE_SIGNTOOL_PATH` are optional path overrides when the provisioned default locations are not
used.

`GITHUB_TOKEN` is supplied by GitHub Actions. It is not a repository secret configured by the
operator. The top-level workflow permission remains `contents: read`; only `publish` declares
`contents: write`.

A prior non-draft GitHub Release with a signed setup installer is also required for the
current-to-previous-to-current qualification sequence. If it is absent, the tag path fails with
`previous_signed_release_missing` or `previous_signed_installer_missing`.

### Manual escape hatch

`workflow_dispatch` with `run_installed_qualification: true` remains non-gating and uses the
existing URL inputs instead of same-run release artifacts:

- `MEMBRANE_QUALIFICATION_INSTALLER_URL` — current signed installer;
- `MEMBRANE_QUALIFICATION_PREVIOUS_INSTALLER_URL` — signed previous installer;
- `MEMBRANE_QUALIFICATION_RELEASE_MANIFEST_URL` — current release manifest;
- `MEMBRANE_QUALIFICATION_SBOM_URL` — current SBOM.

These are GitHub Actions secrets. Missing manual inputs emit the existing typed
`installed_qualification_skipped { reason: prerequisites_absent, detail: ... }` warning and do
not claim qualification. Manual dispatch has no publication job.

## Protected-host boundary

Public CI performs candidate build, candidate checks, and unsigned handoff only. Protected-host
work is signing, finalization, installed-artifact qualification with desktop UI assertions,
GitHub Release publication, and stable bootstrap publication. The workflow does not fake, stub, or
bypass the UI automation checks.

## RightKit ownership caveat

The `finalization`, `installed-qualification`, and `publish` jobs are repo-local additions to a
right-git-managed workflow file. `right-git sync` can drop them. Pending implementation items
§19 and §13.4 move the installed-artifact tag gate into RightKit-owned workflow generation and
regenerate this repository. Until that happens, this gate is load-bearing but not
template-protected.
