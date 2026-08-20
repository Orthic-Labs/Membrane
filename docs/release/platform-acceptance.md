# Platform artifact acceptance

`verify-platform-artifacts.mjs` validates identity-bound receipt pairs without signing, installing, or contacting services. Its closed contract binds receipt ID, commit, release generation, version, platform, artifact name, & SHA-256.

Current acceptance is macOS-only: receipts require passing codesign, notarization,
staple, & Gatekeeper plus install, startup, update, & uninstall outcomes.

`mode: source-ready` records contract readiness only. Only `mode: clean-vm`, with a pseudonymous machine digest & explicit `bypassWarnings: false`, can pass artifact acceptance. Run `node scripts/release/verify-platform-artifacts.mjs CONTRACT.json RECEIPT.json`; retain resulting receipts under `docs/evidence/platform/`.
