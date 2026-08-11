# Add-on release evidence

`scripts/release/evidence/sbom.mjs` hashes root `pnpm-lock.yaml` plus
`engine/Cargo.lock`: exact resolved graphs for Membrane's portable add-on.
`scripts/release/evidence/manifest.mjs` records this SBOM, source provenance,
and declared add-on toolchain beside an immutable sealed release record.

It never builds, signs, uploads, or publishes. Missing artifact, signature,
platform-trust, install, test, or event-history receipts remain explicit gaps.
