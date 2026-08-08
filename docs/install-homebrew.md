# Homebrew install (MBR-904)

Not an install path yet. `brew install --cask membrane` and `brew install membrane`
both fail today by design — see `packaging/homebrew/Casks/membrane.rb` and
`packaging/homebrew/Formula/membrane.rb`, which `raise` an explicit
"intentionally unavailable" error instead of fetching a placeholder URL. This
document describes the generator that turns those files real once RightKit
has produced a signed, sealed release for both macOS architectures — it does
not itself make Homebrew installable.

## Source of truth: RightKit, not a parallel pipeline

Membrane's macOS app is already built, signed, and notarized by RightKit
(`@rightkit/release`), wired into `apps/membrane-hub/package.json` as
`release:build:mac` → `right-release build --platform mac`, configured by
`apps/membrane-hub/right-release.config.mjs`. A real run produces a sealed
release manifest at
`.right-release/sealed/<app>-<version>-<commit8>/mac/release-manifest.json`
(gitignored — local build state, one directory per sealed release) naming
the signed `.dmg`, its SHA-256, and the exact Cloudflare R2 key it will be
uploaded to (`right-release upload --tier patch`).

`scripts/release/homebrew/generate.mjs` is the MBR-904 generator/validator.
It never runs a build, never invokes `brew`, `codesign`, `notarytool`, or any
signing tool, and never writes to git. It only:

1. Reads `apps/membrane-hub/right-release.config.mjs` (dynamic import,
   read-only) for the app name, version, and installer artifact → R2 key map.
2. Reads an already-sealed manifest for one explicit `--commit`, re-verifying
   every file's recorded SHA-256 and size against the bytes on disk before
   trusting anything in it.
3. Joins the sealed manifest's R2 key with the same public download base
   RightKit's own uploader uses (`RIGHTAPPS_PUBLIC_DOWNLOAD_BASE`, default
   `https://pub-6c73208d46c245a9b4881d5e02f6b618.r2.dev` — see
   `tools/rightkit/packages/release/upload-release.mjs`) to derive the
   Homebrew download URL. The URL is never invented; it is always this join.

## Both architectures, always explicit

Homebrew must install cleanly on Intel and Apple Silicon machines. The
generator resolves `mac-arm64` and `mac-x64` independently and only reports
a Cask/Formula as `ready` when **both** resolve to a real URL + SHA-256. A
single architecture's data is never enough to render the real Cask/Formula —
`renderCask`/`renderFormula` fall back to the exact placeholder text in
every other case.

Today:

- **`mac-arm64`** resolves against a real sealed release
  (`apps/membrane-hub/right-release.config.mjs` declares one aarch64 `.dmg`).
- **`mac-x64`** is a declared, validated gap: `right-release.config.mjs`
  declares no x86_64/Intel mac target at all. The generator reports this
  explicitly (`blocked: true`, with a reason) rather than reusing the arm64
  artifact or fabricating an Intel path/hash.
- **The Homebrew Formula** (headless CLI/daemon) is a declared gap for
  *both* architectures: RightKit only builds/signs the full Tauri Hub
  `.app`/`.dmg` for mac (`targets.mac.installer`); there is no
  `targets.mac.headless` tarball target for the `membrane` CLI or
  `crypt-service` daemon to publish standalone yet.

Run `node scripts/release/homebrew/generate.mjs plan --commit <40-hex-sha>`
after a real `right-release build --platform mac` to see the current
resolution (which architectures/artifacts are real vs. blocked, and why)
without writing any file.

## Commands

- `plan --commit SHA [--config PATH] [--sealed-root PATH] [--public-base URL] [--out PATH]`
  — resolves Cask and Formula artifacts from the real config + a real sealed
  manifest for that commit. Prints JSON to stdout, or writes it to `--out`
  only if given (never overwrites an existing file).
- `render --contract PATH [--out-cask PATH] [--out-formula PATH]` — renders
  Cask/Formula Ruby text from a `plan`-shaped JSON file. Falls back to the
  exact committed placeholder unless the contract is fully ready for both
  architectures. Prints to stdout by default; writes only to explicit
  `--out-cask`/`--out-formula` paths (never overwrites).
- `validate [--cask PATH] [--formula PATH] [--contract PATH]` — structurally
  validates Cask/Formula source: no embedded `url`/`sha256`/real `version`
  while not ready, no `:no_check` or `:latest`, and — once ready — an exact
  match against the resolved per-architecture URL/SHA-256 for both
  `mac-arm64` and `mac-x64`.

None of these commands publish anything. Making the Cask/Formula a real
install path — writing over `packaging/homebrew/Casks/membrane.rb` and
`packaging/homebrew/Formula/membrane.rb`, and opening a tap PR — is Adrian's
decision, not this generator's.

## Doctor

`docs/install.md`'s Homebrew source contract already states `brew doctor` is
insufficient on its own: a release doctor must additionally verify artifact
digest, source identity, signatures, and macOS notarization staple before
either package is considered installable. Nothing in this document changes
that requirement.
