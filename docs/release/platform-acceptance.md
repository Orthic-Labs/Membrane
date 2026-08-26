# Platform artifact acceptance

`verify-platform-artifacts.mjs` validates identity-bound receipt pairs without signing, installing, or contacting services. Its closed contract binds receipt ID, commit, release generation, version, platform, artifact name, & SHA-256.

Current acceptance targets Windows x86_64: receipts require passing Authenticode,
trusted timestamp, & publisher identity plus install, startup, update, & uninstall outcomes.

`mode: installed-local` records exact signed-installer lifecycle verification on Windows laptop with `bypassWarnings: false`, including bundled Blueprint inventory, update/rollback, and uninstall residue. No clean-VM or external Blueprint provisioning gate applies. Run `node scripts/release/verify-platform-artifacts.mjs CONTRACT.json RECEIPT.json`; retain resulting receipts under `docs/evidence/platform/`.

`pnpm qualification:write-windows-platform-evidence -- --qualification
QUALIFICATION.json --release RELEASE-IDENTITY.json --out-contract CONTRACT.json
--out-receipt RECEIPT.json` performs deterministic qualification-to-platform
projection & validates pair before writing immutable files.
