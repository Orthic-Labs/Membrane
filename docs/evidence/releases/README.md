# Release evidence storage

Store one immutable `RELEASE.json` plus referenced local receipts per release generation. Do not add fabricated, unsigned, or credential-bearing material. `verify-release-evidence.mjs` accepts only hash-bound manifests with installed macOS & Windows vector-dispatch receipts, source/SBOM/provenance/toolchain/test evidence, signature/platform-trust identity & explicit event-history status.
