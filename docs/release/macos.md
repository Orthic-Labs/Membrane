# macOS release contract

Source preparation only. A release names one exact commit, version, `.app`, and `.dmg`; receipt records both SHA-256 values. `scripts/release/macos/release-macos.mjs` is fail-closed and never publishes.

Run `verify` for strict `codesign` & `hdiutil` checks, `notarize` with an explicit keychain profile for `xcrun notarytool submit --wait`, then `staple` for both artifacts. Run `receipt` with `--commit`, `--version`, and a new `--out` path only after all checks pass. Missing paths, extensions, profile, or receipt fields stop execution.

Artifact acceptance still requires a real signed/notarized/stapled app and DMG, matching receipt hashes, and external distribution approval.
