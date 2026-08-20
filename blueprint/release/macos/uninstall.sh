#!/bin/sh
# D17: uninstall — complete and data-preserving by default.
set -e
INSTALL_LOCATION="${INSTALL_LOCATION:-/usr/local/lib/cortex}"
if [ -d "$INSTALL_LOCATION" ]; then
  rm -rf "$INSTALL_LOCATION"
fi
echo "Cortex uninstalled (user data under ~/.cortex preserved)."
