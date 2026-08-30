# Release evidence manifest

`dist/release/evidence/release-evidence.v1.schema.json` defines offline, hash-bound evidence. `node scripts/release/verify-release-evidence.mjs --manifest docs/evidence/releases/RELEASE.json` recomputes every listed file hash before accepting source, SBOM, provenance, toolchain, test, compatibility, signature, platform-trust, installed-platform & event-history receipts.

Each manifest binds immutable tag, source commit/tree, release generation, target, artifact SHA-256 & `CORTEX_VECTOR_DISPATCH_V2`. Current Windows x86_64 evidence requires an Ed25519 receipt plus Authenticode, with receipt identities bound to identical artifact bytes. One installed Windows entry is a hash-bound pair validated through `verify-platform-artifacts.mjs`, not an opaque claim. Installed runtime inventory and SBOM entries must bind exact staged bytes, including pinned Blueprint component bytes; update/rollback and uninstall receipts remain part of same evidence chain. Explicit `sealed` or `legacy-unsealed` event-history status is required. Evidence paths are root-confined regular files.

Verifier performs no build, signing, platform verification, installation, upload, or publication. Real Windows tools produce receipts separately; placeholder or modified files fail closed.

After local RightKit signing, generate SBOM, then run
`pnpm qualification:prebind-windows-release` to create installer/SBOM binding
consumed by installed qualification. Convert resulting qualification receipt with
`pnpm qualification:write-windows-platform-evidence`; assemble final
`RELEASE.json` with Hub `release:evidence:win`; verify it, then issue native-only
seal. Provisional binding is marked `provisional: true` & cannot pass final
release verifier.
