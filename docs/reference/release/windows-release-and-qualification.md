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
  -> candidate: native Windows/macOS build -> check -> upload unsigned candidates
  -> windows-sign: Azure OIDC signing -> finalized Windows handoff
  -> macos-sign: Developer ID signing + notarization -> finalized macOS handoff
  -> installed-qualification: protected Windows host installs same-run signed Windows handoff
  -> publish: protected Windows host publishes only after qualification + macOS finalization
```

`candidate` is public CI. It builds exact Windows x86_64 and macOS arm64 unsigned candidates.
Neither handoff is a customer artifact.

`windows-sign` runs in GitHub's protected `release` environment, authenticates to Azure with
short-lived OIDC, consumes exact Windows candidate bytes, and emits signed installer plus
portable-release inputs. `macos-sign` runs in that same protected environment, materializes
Apple credentials in a temporary keychain, signs and notarizes its exact macOS candidate.
RightKit owns both generated signing jobs; their manifest configuration is `.rightgit.json`.

`installed-qualification` runs on configured self-hosted Windows runner. On a tag it
uses `actions/download-artifact` to obtain same-run Windows signing output. It obtains
the newest prior stable-layout release's installer for the downgrade leg, then runs
`scripts/qualification/install-release.ps1`. Missing signing, runner, prior-release, installer,
manifest, or SBOM prerequisites fail the tag path with the typed
`installed_qualification_skipped { reason: prerequisites_absent, detail: ... }` reason. The job
never converts a hosted runner or an unsigned artifact into a pass.

The script exercises the installed installer through install, startup, tray/UI automation,
Blueprint checks, downgrade, upgrade, state continuity, uninstall, and residue checks. Release
`0.1.18` is the one stable-layout migration exception: qualification performs clean install plus
same-version repair into a distinct immutable version root. From `0.1.19`, prior signed
stable-layout installer is mandatory. The
product under test is the installed artifact. The development workspace is for development and
testing only; it is never the thing a customer runs.

`publish` runs only after `installed-qualification` succeeds. It has the only job-level
`contents: write` permission. RightKit publishes the signed portable release and stable bootstrap
through GitHub Releases; the workflow then attaches the signed
installer, qualification manifest, SBOM, and qualification evidence to the GitHub Release. A
failed qualification cannot reach publication.

## Required configuration

### Tag path

| Kind | Exact name | What it enables | What breaks without it |
|---|---|---|---|
| Repository variable | `MEMBRANE_QUALIFICATION_RUNNER_LABEL` | Installed qualification and publication on a self-hosted Windows runner with interactive desktop | Qualification fails; tray UI assertions cannot move to hosted runner |
| Release environment variable | `AZURE_ARTIFACT_SIGNING_ENDPOINT` | Azure Artifact Signing endpoint | Windows signing fails |
| Release environment variable | `AZURE_ARTIFACT_SIGNING_ACCOUNT` | Azure Artifact Signing account | Windows signing fails |
| Release environment variable | `AZURE_ARTIFACT_SIGNING_PROFILE` | Azure Artifact Signing certificate profile | Windows signing fails |
| GitHub Pages environment | `github-pages` | Stable custom-domain bootstrap deployment | Pages deployment fails without configured environment and DNS |

Azure identity uses OIDC through release-environment variables `AZURE_CLIENT_ID`,
`AZURE_TENANT_ID`, and `AZURE_SUBSCRIPTION_ID`; GitHub-hosted signing installs pinned signing
tools. macOS signing uses scoped `APPLE_CERTIFICATE_BASE64`, `APPLE_CERTIFICATE_PASSWORD`,
`APPLE_KEYCHAIN_PASSWORD`, `APPLE_API_KEY_BASE64`, `APPLE_API_KEY`, `APPLE_API_ISSUER`, and
`APPLE_DEVELOPER_ID` release-environment configuration.

`GITHUB_TOKEN` is supplied by GitHub Actions. It is not a repository secret configured by the
operator. The top-level workflow permission remains `contents: read`; only `publish` declares
`contents: write`.

A prior non-draft GitHub Release at `0.1.18` or later with a signed setup installer is required
from `0.1.19` onward for the current-to-previous-to-current qualification sequence. If absent,
tag path fails with
`previous_signed_release_missing` or `previous_signed_installer_missing`.

## Protected-host boundary

Public CI performs native candidate build plus tag-gated Windows/macOS signing. Protected-host
work is installed-artifact qualification with desktop UI assertions, GitHub Release publication,
and stable bootstrap publication. The workflow does not fake, stub, or bypass UI automation.

## RightKit ownership

RightKit generates candidate, signing, installed-qualification, and publication DAG from
`.rightgit.json`. `right-git drift .` verifies rendered workflow integrity.
