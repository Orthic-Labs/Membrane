# macOS release

## Canonical path: RightKit

RightKit already provides signing, notarization, hardening, and sealing for both macOS and Windows, and it is already wired into this app — see `apps/membrane-hub/right-release.config.mjs` (`targets.mac`) and `apps/membrane-hub/package.json`'s `release:doctor` / `release:build:mac` scripts. The pipeline in this directory does not duplicate any of that. It delegates to RightKit and only verifies what RightKit actually produced.

Run, from `apps/membrane-hub`:

```
pnpm release:doctor           # right-release doctor --platform mac
pnpm release:build:mac        # right-release build --platform mac
```

`right-release build --platform mac` (`tools/rightkit/packages/release/build-release.mjs`, driving `release.mjs`):

1. runs `pnpm run rightkit:package:mac` (`apps/membrane-hub/scripts/build-mac-release.mjs`), which:
   - runs `tauri build --bundles app,dmg`. Tauri reads `apps/membrane-hub/src-tauri/tauri.conf.json#bundle.macOS.signingIdentity` and codesigns every embedded binary declared in `bundle.externalBin` (the `crypt-service` daemon, the `membrane` CLI/MCP sidecar) and the outer `Membrane Hub.app`, inner-to-outer, with hardened runtime — Apple requires hardened runtime for notarization to succeed at all, so a successful notarization already proves it;
   - runs `xcrun notarytool submit <dmg> --wait` (authenticated via `@rightkit/release/notary-auth.mjs`'s `notarytoolAuthArgs()`: the `apple-dev-notary` keychain profile, or the `APPLE_API_KEY_PATH` / `APPLE_API_KEY` / `APPLE_API_ISSUER` triplet);
   - runs `xcrun stapler staple` + `xcrun stapler validate` on the DMG;
   - runs `spctl -a -vv --type open --context context:primary-signature` (Gatekeeper) on the DMG;
2. runs RightKit's `hardeningscan.mjs` over the built artifacts (leaked local paths, secrets, debug symbols, source trees);
3. seals the result under `.right-release/sealed/<app>-<version>-<commit8>/mac/`, writing a `release-manifest.json` that binds the DMG's sha256 and the build's checkpoints (`preflight_complete`, `build_complete`, `signed`, `hardened`, `sealed`).

Nothing in `scripts/release/macos/**` re-implements any of the above. There is no bespoke codesign loop, no bespoke `notarytool submit`, and no bespoke `stapler staple` in this repository — RightKit and Tauri already own signing identity, entitlements application, hardened runtime, notarization submission, and DMG stapling.

## What this repository still owns: acceptance evidence

MBR-901's acceptance is "codesign, spctl, notarization log, stapler validation, fresh install, and update tests pass." Two things RightKit's build does not do on its own are needed to satisfy that literally, and `scripts/release/macos/` supplies exactly those two, nothing more:

1. **Independent re-verification of RightKit's real output.** `node scripts/release/macos/release-macos.mjs verify --app <path>.app --dmg <path>.dmg [--sealed-dir <path>]` re-checks, read-only, what the build already produced: `codesign --verify --deep --strict`, the hardened-runtime flag on `codesign --display --verbose=4`, `spctl --assess --type execute` (Gatekeeper on the `.app`), `hdiutil verify` (DMG integrity), `spctl --assess --type open --context context:primary-signature` (Gatekeeper on the DMG), and `xcrun stapler validate` on both the `.app` and the DMG. None of these mutate, sign, notarize, or staple anything — `stapler validate` checks an existing ticket, it does not create one. Passing `--sealed-dir <path>` (e.g. `.right-release/sealed/membrane-hub-<version>-<commit8>/mac`) additionally cross-checks that `--dmg` is byte-for-byte the exact installer RightKit sealed for this commit and version, via `scripts/release/macos/contract.mjs`'s `readSealedReleaseManifest` / `verifySealedInstallerHash` — so this command verifies RightKit's real sealed artifact, not an arbitrary same-named file.

2. **A notarization log, persisted.** RightKit's build submits and waits, but does not save the notarization submission ID or log anywhere durable. `node scripts/release/macos/release-macos.mjs notarization-log --submission-id <uuid> --out <path>` runs `xcrun notarytool log <submission-id>` (find the ID with `xcrun notarytool history --keychain-profile apple-dev-notary`, or from the build's own stdout) and writes it to a confined, non-overwritable path — the notarization-log evidence MBR-901's acceptance criterion names. This uses the same credential shape as the real build (`--keychain-profile apple-dev-notary` by default, or the `APPLE_API_KEY_PATH`/`APPLE_API_KEY`/`APPLE_API_ISSUER` triplet); `scripts/release/macos/contract.mjs`'s `notarytoolAuthArgs()` mirrors `@rightkit/release/notary-auth.mjs`'s function of the same name byte-for-byte in behavior. It cannot import that function directly: this membrane repository is its own standalone Git checkout with no workspace or file link to `tools/rightkit` (RightKit's own rules require apps to consume it as a published npm package, never a local path), and the membrane repo root itself has no `@rightkit/release` dependency — only `apps/membrane-hub` does, for its own build script. Mirroring this ~15-line, non-signing, credential-*argument-shape* helper is a repo-boundary necessity, not a duplicate of the actual signing/notarizing/stapling actions, which this repository does not reimplement anywhere.

## Commands

- `verify --app <path>.app --dmg <path>.dmg [--sealed-dir <path>]` — the read-only re-verification described above.
- `notarization-log --submission-id <uuid> [--keychain-profile <name>] --out <path>` — captures the notarization log.
- `receipt --app <path>.app --dmg <path>.dmg --commit <sha> --version <semver> --out <path>` — runs the same read-only checks as `verify`, then writes an immutable `orthic.membrane.macos-release-receipt.v1` binding the exact commit, version, and both artifact hashes.
- `verify-receipt --app <path>.app --dmg <path>.dmg --receipt <path>` — validates a previously written receipt against the artifacts on disk (schema, checks, and hash match). No tool invocation.
- `platform-receipt --out <path> --mode clean-vm --commit <sha> --version <semver> ...` — assembles the cross-platform `orthic.membrane.platform-acceptance.v1` receipt (trust: `codesign`/`notarization`/`staple`/`gatekeeper`; lifecycle: `install`/`startup`/`update`/`uninstall` — the fresh-install and update-test acceptance evidence) by reusing the shared validator in `scripts/release/verify-platform-artifacts.mjs` (imported, never modified), so a macOS receipt produced here satisfies the same schema `scripts/release/verify-release-evidence.mjs` checks at the book gate. This is the receipt format `MBR-904` and `MBR-808` consume; its schema and CLI shape are unchanged by this task.

Missing paths, a malformed submission ID, an incomplete receipt input, or a `--sealed-dir` mismatch all stop execution before any tool that could sign, notarize, or staple anything runs — every code path here is either read-only against an already-built artifact, or a pure JSON validator.

## Entitlements

`packaging/macos/entitlements.plist` grants **no** hardened-runtime exception entitlement — see the rationale comment in that file (same Developer ID team signs every embedded binary; the Hub renders via WKWebView, not an in-process JS engine; sidecar↔Hub traffic stays on loopback/stdio per the Membrane "loopback-bound" invariant). It is currently a rationale document, not a wired build input: `apps/membrane-hub/src-tauri/tauri.conf.json#bundle.macOS.entitlements` is unset, so Tauri applies no custom entitlements file at all — behaviorally identical to this file's empty `<dict/>`. `apps/membrane-hub/**` is owned by a concurrent lane and out of scope here; wire this file in via that key, and update its rationale comment, the day a concrete hardened-runtime failure actually needs an exception.

## Credentials

Referenced by name only, never by value, anywhere in this pipeline:

- `apple-dev-notary` — the notarytool keychain profile name `@rightkit/release/notary-auth.mjs` defaults to (holds the Apple ID / app-specific password, or is superseded by the API-key triplet below).
- `APPLE_API_KEY_PATH` / `APPLE_API_KEY` / `APPLE_API_ISSUER` — App Store Connect API key auth for notarytool, read straight from the environment by RightKit's build and by this repository's `notarytoolAuthArgs()` mirror; never read, printed, or stored beyond that.
- `apps/membrane-hub/src-tauri/tauri.conf.json#bundle.macOS.signingIdentity` — the Developer ID Application identity name (a public certificate common name, not a secret; the private key stays in the signer's keychain). Overridable via `APPLE_SIGNING_IDENTITY` in `apps/membrane-hub/scripts/build-mac-release.mjs`.

## Acceptance still pending a real, credentialled run

Acceptance still requires a real `pnpm release:build:mac` run on a machine holding the Developer ID signing identity and a notarization credential, producing a real sealed `.app`/`.dmg`, a real `notarytool log`, a fresh clean-VM install, and an update test. This task ships the acceptance/verification pipeline and its deterministic tests; it does not execute `right-release`, `tauri`, `codesign`, `spctl`, `hdiutil`, `xcrun`, or `stapler`, and does not claim acceptance pass — see `evidence/productization/MBR-901/`.
