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

# Owner-gated protected values (S-14).
: "${APPLE_TEAM_ID:?APPLE_TEAM_ID required}"
: "${APPLE_DEVELOPER_ID_APPLICATION:?APPLE_DEVELOPER_ID_APPLICATION required}"
: "${APPLE_DEVELOPER_ID_INSTALLER:?APPLE_DEVELOPER_ID_INSTALLER required}"
: "${APPLE_NOTARY_KEY_ID:?APPLE_NOTARY_KEY_ID required}"
: "${APPLE_NOTARY_ISSUER_ID:?APPLE_NOTARY_ISSUER_ID required}"
: "${APPLE_NOTARY_KEY_P8_BASE64:?APPLE_NOTARY_KEY_P8_BASE64 required}"

VERSION="${VERSION:-0.2.0}"
INSTALL_LOCATION="${INSTALL_LOCATION:-/usr/local/lib/cortex}"

# Sign every Mach-O in the staged bundle.
find "$STAGE" -type f -perm -111 -print0 | while IFS= read -r -d '' file; do
  /usr/bin/codesign --force --options runtime --timestamp --sign "$APPLE_DEVELOPER_ID_APPLICATION" "$file"
done
/usr/bin/codesign --verify --deep --strict --verbose=2 "$STAGE"

# Build and sign a per-user PKG.
pkgbuild --root "$STAGE" --identifier io.orthic.cortex --version "$VERSION" --install-location "$INSTALL_LOCATION" component.pkg
productbuild --distribution release/macos/distribution.xml --package-path . --sign "$APPLE_DEVELOPER_ID_INSTALLER" "Cortex-$VERSION.pkg"

# Notarize and staple.
KEY_FILE="$(mktemp)"
trap 'rm -f "$KEY_FILE"' EXIT
echo "$APPLE_NOTARY_KEY_P8_BASE64" | base64 --decode > "$KEY_FILE"
xcrun notarytool submit "Cortex-$VERSION.pkg" --key "$KEY_FILE" --key-id "$APPLE_NOTARY_KEY_ID" --issuer "$APPLE_NOTARY_ISSUER_ID" --wait
xcrun stapler staple "Cortex-$VERSION.pkg"
spctl --assess --type install --verbose=4 "Cortex-$VERSION.pkg"
pkgutil --check-signature "Cortex-$VERSION.pkg"

echo "signed+notarized: Cortex-$VERSION.pkg"
