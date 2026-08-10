#!/bin/bash
# D17: sign and notarize macOS release artifacts (S-14 order).
# Requires owner variables; never prints certificate or API-key material.
# --verify-only checks an existing staged/signed artifact without signing.
set -euo pipefail

STAGE="${1:-}"
MODE="${2:-sign}"

if [[ "$MODE" == "--verify-only" ]]; then
  if [[ -z "$STAGE" ]]; then echo "usage: sign-and-package.sh <path> --verify-only" >&2; exit 2; fi
  /usr/bin/codesign --verify --deep --strict --verbose=2 "$STAGE"
  if [[ -f "$STAGE" && "$STAGE" == *.pkg ]]; then
    pkgutil --check-signature "$STAGE"
    spctl --assess --type install --verbose=4 "$STAGE"
  fi
  echo "verify-only OK: $STAGE"
  exit 0
fi

# Signing is owned by RightKit `right-release` from the primary checkout.
# This repository MUST NOT perform codesign/pkgbuild/productbuild/notarytool
# directly — see docs/rules/release-signing.md and EC v4 D-18.
# The only supported path here is --verify-only (above). Any direct signing
# attempt now fails closed and points to the owned capability.
echo "ERROR: in-repo macOS signing is forbidden — use \`right-release\` from the primary checkout (see docs/rules/release-signing.md, docs/operations/release.md)" >&2
echo "Hint: \`right-release build\` handles codesign → pkgbuild → productbuild → notarytool → stapler → spctl → pkgutil per S-14, with credentials never in CI." >&2
exit 1
