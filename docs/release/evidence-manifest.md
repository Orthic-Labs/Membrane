# Release evidence manifest

`dist/release/evidence/release-evidence.v1.schema.json` defines offline, hash-bound evidence. `node scripts/release/verify-release-evidence.mjs --manifest docs/evidence/releases/RELEASE.json` recomputes every listed file hash before accepting source, SBOM, provenance, toolchain, test, compatibility, signature, platform-trust, installed-platform & event-history receipts.

Each manifest binds immutable tag, source commit/tree, release generation, target, artifact SHA-256 & `CORTEX_VECTOR_DISPATCH_V2`. Current Mac evidence requires an Ed25519 receipt plus Apple notarization, with receipt identities bound to identical artifact bytes. One installed macOS entry is a hash-bound pair validated through MBR-806, not an opaque claim. Explicit `sealed` or `legacy-unsealed` event-history status is required. Evidence paths are root-confined regular files.

Verifier performs no build, signing, notarization, installation, upload, or publication. Real platform tools produce receipts separately; placeholder or modified files fail closed.
