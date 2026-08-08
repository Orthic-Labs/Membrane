# Transactional update

`membrane::update` requires a finite supervisor quiesce, verified staging, atomic
directory activation, schema migration, & atomic receipt publication. Cancellation
or any deterministic phase fault restores prior active release & preserves staged release.

`UpdateHooks::quiesce`, verification, & migration must be finite supervisor operations;
the engine samples cancellation before each phase. Migration implementers must make
`rollback_schema` undo both partial & complete schema changes; failures report all
schema/filesystem rollback errors while still attempting every restoration step.
Receipt publication is last & atomic. A pre-existing `.rollback` path is rejected;
post-success cleanup failure is returned so operators can repair it deterministically.

## Dual-signature verification (MBR-911)

Staging (above) is necessary but not sufficient: an update must also *admit*
under two independent signatures before verified staging is allowed to begin.
`engine/crates/membrane-updater` is that admission gate. It is pure and has no
filesystem, process, download, migration, or activation API — it cannot itself
stage or activate anything in the transactional flow above; it only decides
`Ok(VerifiedUpdate)` or `Err(BlockedUpdate)`.

**Two independent trust domains**, matching what RightKit's own release
pipeline already signs for Membrane Hub
(`apps/membrane-hub/right-release.config.mjs`, `targets.win.updater`):

1. **Tauri updater signature.** Produced by RightKit's
   `tools/rightkit/packages/release/sign-updater.mjs` (and, for macOS,
   `create-mac-updater.mjs`), which runs `pnpm exec tauri signer sign`
   against the shared `rightsuite-updater-key` and writes `<artifact>.sig`.
   `tools/rightkit/packages/release/publish-update.mjs` folds that signature
   into the update manifest as `platforms[<target>] = { signature, url }`.
   Tauri's own updater plugin verifies that signature against the embedded
   pubkey when it parses this manifest at update-check time.
2. **Platform trust.** The `orthic.membrane.platform-acceptance.v1` receipt
   already defined by `scripts/release/verify-platform-artifacts.mjs` and
   built per platform by `scripts/release/macos/contract.mjs` /
   `scripts/release/windows/contract.mjs`: codesign + notarization + staple +
   Gatekeeper on macOS, Authenticode + Public Trust + RFC3161 timestamp on
   Windows.

This is a deliberate design choice, not an oversight: **neither the crate nor
its adapters re-implement or re-verify signature cryptography.** RightKit
already owns signing on both platforms (see
`tools/rightkit/packages/release/{sign-updater,create-mac-updater,notary-auth,
asr-artifact-adoption,hardeningscan}.mjs` and the CLI at
`tools/rightkit/packages/release/cli/right-release.mjs`); this task plugs into
that output. `apps/membrane-hub/src-tauri/src/update_admission.rs` is the
concrete adapter: it parses RightKit's real update-manifest entry and
platform-acceptance receipt, binds both to one artifact SHA-256, and calls
`membrane_updater::verify`. `scripts/release/updater/contract.mjs` is the
same admission logic at release-orchestration time, reusing (importing, not
duplicating) `scripts/release/verify-platform-artifacts.mjs`.

**Fails closed** on every path:

| Case | Where it is caught |
| --- | --- |
| Missing/empty Tauri signature | `identity_valid` (crate) / `validateUpdaterManifestEntry` (contract) |
| Unknown/untrusted signing key | the trusted adapter's `verify_tauri` rejects it |
| One valid signature out of two | `verify` still requires both `verify_tauri` and `verify_platform` to pass |
| Downgrade (`to` not a strictly greater `major.minor.patch` than `from`) | `is_upgrade` / `isUpgrade`, checked before either trust domain runs |
| Mismatched artifact hash across the two evidence records | `identity_valid` / the manifest-vs-receipt SHA-256 bind |

A `BlockedUpdate` carries a stable failure-code list, the version pair, the
artifact hash, and `repair_path: "repair/update-signatures"` — enough to
diagnose without ever re-exposing key material. Because the crate has no
activation API, a blocked update cannot have mutated anything: the current
version is preserved by construction, matching the transactional-update
guarantee above (a rejected admission never reaches the quiesce/staging
phases at all).

Only RightKit's macOS updater artifact remains unpublished as of this task
(`packaging/macos/bundle-manifest.json`'s `updater` component is
`present: false`, owned by the macOS release lane) — the moment RightKit's
`right-release.config.mjs` declares a `targets.mac.updater` artifact, the
same platform-agnostic `Platform::Macos` path in
`engine/crates/membrane-updater` and `update_admission.rs` covers it with no
further code changes.
