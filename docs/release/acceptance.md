# Platform signing and installed-path acceptance (MBR-806)

This is the acceptance layer above `scripts/release/macos/**` (MBR-901) and
`scripts/release/windows/**` (MBR-902). Those two deliver, per platform: (a)
a read-back of RightKit's real sealed build output, and (b) the
membrane-specific evidence RightKit does not produce on its own — an
install/startup/update/uninstall lifecycle receipt, and, on Windows, a
per-inner-PE signing proof. `scripts/release/acceptance/**` does not
duplicate either of those. It is the single fail-closed gate that **binds
them together** and, on Windows, closes a gap the two lower layers leave
open when consumed separately.

## Why a binding layer is needed, not just the two existing layers

`scripts/release/verify-platform-artifacts.mjs` validates the shared
`orthic.membrane.platform-acceptance.v1` receipt schema — the one the book
gate's `node scripts/release/verify-platform-artifacts.mjs --all` actually
checks. Its `trust` block for Windows is `{ authenticode, publicTrust,
rfc3161 }`: three pass/fail flags about the **outer installer only**. That
schema has no field for per-payload proof — see
`packaging/contracts/platform-acceptance.v1.json`, `trust` for Windows is a
flat three-key object.

`scripts/release/windows/contract.mjs`'s `validateLifecycleReceipt`
separately proves every inner PE (main binary + Tauri `bundle.externalBin`
sidecars) was individually `signtool verify /pa /tw`-checked — but it
validates a **different** schema (`windows-installer-receipt.v1`), never
touched by `verify-platform-artifacts.mjs`.

Consumed independently, a Windows release could present a fully valid
`orthic.membrane.platform-acceptance.v1` receipt (`trust.authenticode:
"pass"`, attesting only the outer NSIS installer) and pass the book gate's
`--all` check, while every inner PE the installer actually runs —
`membrane-hub.exe`, `crypt-service.exe`, `membrane.exe` — remains
unsigned. This is not hypothetical: `docs/release/windows.md` records it as
a confirmed defect. RightKit signs `targets.win.sign.files` strictly
**after** `tauri build --bundles nsis` has already embedded those binaries
into the installer (`src-tauri/windows/installer.nsi`'s "Copy main
executable" / "Copy external binaries" steps), so signing files on disk
afterward never reaches the copies already inside the installer.

`scripts/release/acceptance/windows-installed-acceptance.mjs` closes this
by requiring **both** documents, cross-bound by commit/installer identity,
before accepting anything:

1. The shared platform-acceptance receipt (outer trust + full lifecycle).
2. RightKit's sealed `release-manifest.json` plus a `windows-installer-receipt.v1`
   proving every one of `minimumExpectedPayloadCount` (Tauri's
   `bundle.externalBin.length + 1`, read from `tauri.conf.json`) inner PEs
   individually.

If #2 is missing, incomplete, or names fewer PEs than Tauri actually
bundles — or any named PE's `verify` is not `"pass"` — the gate throws.
`trust.authenticode: "pass"` on #1 alone is never sufficient. See
`tests/platform/acceptance/windows-installed-acceptance.test.mjs`, in
particular "FAILS CLOSED when the outer installer is signed but one
inner-PE payload was never verified (the confirmed RightKit NSIS gap)".

## macOS

Tauri/RightKit codesign every `bundle.externalBin` sidecar and the outer
`.app` inner-to-outer, with hardened runtime, before notarization — so
there is no equivalent outer/inner split on macOS
(`docs/release/macos.md`). `scripts/release/acceptance/macos-installed-acceptance.mjs`
still fails closed on the ways a "signed" claim could otherwise be hollow:
the shared platform-acceptance receipt's trust/lifecycle fields must be
complete; RightKit's sealed `release-manifest.json` must be read back and
its installer hash must match the `--dmg` bytes on disk
(`readSealedReleaseManifest` / `verifySealedInstallerHash`, imported from
`scripts/release/macos/contract.mjs`, never re-signed here); and the
receipt's commit/artifact identity must match. It also re-verifies the
macOS release receipt (`orthic.membrane.macos-release-receipt.v1`) against
the actual `.app`/`.dmg` bytes by spawning
`scripts/release/macos/release-macos.mjs verify-receipt` — reused, not
reimplemented, because that script's directory-hash walk is internal to
it. `verify-receipt` never touches `codesign`/`spctl`/`hdiutil`/`xcrun`; it
only compares already-recorded hashes to files already on disk.

## Commands

```
node scripts/release/acceptance/windows-installed-acceptance.mjs \
  <platform-receipt.json> <release-manifest.json> <expected-payloads.json> <lifecycle-receipt.json> [tauri.conf.json]

node scripts/release/acceptance/macos-installed-acceptance.mjs \
  <platform-receipt.json> <sealed-dir> <app-path> <dmg-path> <macos-receipt.json>
```

Both exit non-zero and print a `FAIL CLOSED` (or the underlying validator's
own fail-closed message) on any missing, mismatched, or incomplete
evidence — never "unknown, assume pass". Neither invokes `codesign`,
`notarytool`, `spctl`, `hdiutil`, `xcrun`, `stapler`, `signtool`, or
`AzureSignTool`; both only validate JSON already produced by the real,
credentialled RightKit build (`pnpm release:build:mac` /
`pnpm release:build:win`) and by a real clean-VM install/startup/
update/uninstall pass. Missing paths or a `--dmg`/installer mismatch stop
execution before any such tool would run, in either layer they compose.

## Test suite

- `tests/platform/acceptance/windows-installed-acceptance.test.mjs` —
  14 tests: a full pass, the confirmed inner-PE gap (both "one payload
  unverified" and "payload entirely absent"), too-few proven payloads,
  a missing lifecycle receipt, `source-ready` masquerading as installed,
  bypass warnings, incomplete Windows trust, and identity-mismatch
  (artifact sha256 / commit) between the outer receipt and RightKit's
  sealed manifest — at both the function and real-subprocess CLI level.
- `tests/platform/acceptance/macos-installed-acceptance.test.mjs` —
  9 tests: a full pass, a tampered sealed DMG, an identity-mismatched
  artifact, a macOS receipt whose recorded hash does not match the actual
  `.app`/`.dmg` bytes, a macOS receipt with an incomplete check, a missing
  macOS receipt file, incomplete macOS trust, `source-ready` masquerading
  as installed, and a decoy DMG with matching bytes but the wrong sealed
  path.

Run with `node --test tests/platform/acceptance/windows-installed-acceptance.test.mjs`
and `node --test tests/platform/acceptance/macos-installed-acceptance.test.mjs`.
Neither test file spawns a real signing tool; the Windows suite is pure-JSON
plus real-subprocess CLI runs of this task's own scripts, and the macOS
suite uses real temp-directory fixtures plus `release-macos.mjs
verify-receipt` (hash comparison only).

## Acceptance still pending a real, credentialled run

This task ships the acceptance test suite and the two fail-closed gate
scripts; it does not execute `right-release`, `tauri`, `codesign`,
`notarytool`, `spctl`, `signtool`, or `AzureSignTool`, and does not claim a
passing clean-VM run occurred. Real acceptance — clean macOS and Windows
VMs, real signed/notarized/stapled artifacts, and a real
`windows-installed-acceptance.mjs` / `macos-installed-acceptance.mjs` run
against them — is deferred to the Book 3 final phased gate, per
`MEMBRANE-BOOK-MODE-EXECUTION-RULES.md`. See
`evidence/productization/MBR-806/acceptance.json`.
