# Source release pipeline

MBR-903 defines a manually invoked, source-only release plan for mac arm64/x64 & Windows x64/arm64. `scripts/release/orchestrate-release.mjs` validates one manifest, then prints JSON; it never builds, signs, notarizes, verifies bytes, uploads, or publishes.

Manifest identity is immutable: `vX.Y.Z` tag, 40-character commit, tree digest, release generation, supported target, artifact SHA-256, evidence SHA-256, `CRYPT_VECTOR_DISPATCH_V2`, and `publish: false`. Ordered stages are provenance → SBOM → sign → platform trust → verify. Each stage carries planned status plus evidence digest. mac targets require `apple-notary`; Windows targets require `windows-authenticode`. Windows arm64 remains conformance-only until an installed receipt promotes it.

Run `node scripts/release/orchestrate-release.mjs --manifest path/to/manifest.json` to print a plan, or add `--validate` for normalized validation output. Platform builders and signing services must produce real receipts before any separate artifact gate; this contract itself is not artifact acceptance.
