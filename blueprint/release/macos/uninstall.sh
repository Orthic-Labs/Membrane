#!/bin/sh
# D17: uninstall — complete and data-preserving by default.
set -e
INSTALL_LOCATION="${INSTALL_LOCATION:-/usr/local/lib/blueprint}"
if [ -d "$INSTALL_LOCATION" ]; then
  rm -rf "$INSTALL_LOCATION"
fi
echo "Blueprint uninstalled (user data under ~/.blueprint preserved)."
