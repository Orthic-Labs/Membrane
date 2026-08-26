# Release evidence storage

Store one immutable `RELEASE.json` plus referenced local receipts per release generation. Do not add fabricated, unsigned, or credential-bearing material. `verify-release-evidence.mjs` accepts only hash-bound manifests with an installed Windows x86_64 vector-dispatch receipt, source/SBOM/provenance/toolchain/test evidence, signature/platform-trust identity & explicit event-history status. The manifest must bind one exact installer digest plus inventory/SBOM/update/uninstall receipts; Homebrew/WinGet publication is not part of this seal.
