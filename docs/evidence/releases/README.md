# Release evidence storage

Store one immutable `RELEASE.json` plus referenced local receipts per release generation. Do not add fabricated, unsigned, or credential-bearing material. `verify-release-evidence.mjs` accepts only hash-bound manifests with an installed Windows x86_64 vector-dispatch receipt, source/SBOM/provenance/toolchain/test evidence, signature/platform-trust identity & explicit event-history status. The manifest must bind one exact installer digest plus inventory/SBOM/update/uninstall receipts; Homebrew/WinGet publication is not part of this seal.

## What is not stored here

The installer, its signature and the full installed-qualification transcript are
not committed. They are reproducible from the release they attest to: the
GitHub release carries the signed artifacts, and the workflow run carries the
qualification evidence. `RELEASE.json` binds them by digest, which is what
`verify-release-evidence.mjs` checks, so the bytes themselves add nothing but
weight; the 0.1.12 installer alone cost 50 MB in every clone of this
repository. `.gitignore` refuses them.
