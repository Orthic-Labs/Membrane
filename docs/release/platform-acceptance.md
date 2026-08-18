# Platform artifact acceptance

`verify-platform-artifacts.mjs` validates identity-bound receipt pairs without signing, installing, or contacting services. Its closed contract binds receipt ID, commit, release generation, version, platform, artifact name, & SHA-256.

macOS receipts require passing codesign, notarization, staple, & Gatekeeper. Windows receipts require passing Authenticode, Public Trust, & RFC3161 timestamp validation. Both require install, startup, update, & uninstall outcomes.

`mode: source-ready` records contract readiness only. Only `mode: clean-vm`, with a pseudonymous machine digest & explicit `bypassWarnings: false`, can pass artifact acceptance. Run `node scripts/release/verify-platform-artifacts.mjs CONTRACT.json RECEIPT.json`; retain resulting receipts under `docs/evidence/platform/`.
