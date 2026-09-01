// D16: GitHub-release update channels and install-owner detection.
// Installed archives require a signed manifest and matching checksum.

import { existsSync } from "node:fs";
import { dirname, join, resolve } from "node:path";
import { fileURLToPath } from "node:url";

export const CHANNELS = Object.freeze(["stable", "beta", "nightly"]);
const SCRIPT_DIR = dirname(fileURLToPath(import.meta.url));

function installationRoot() {
  return resolve(SCRIPT_DIR, "..", "..", "..");
}

function hasPortableLayout(root) {
  const embeddedNode = join(root, "lib", "node");
  const packagedApplication = join(root, "app", "package");
  return existsSync(embeddedNode) && existsSync(packagedApplication);
}

function updateChecksDisabled() {
  return process.env.BLUEPRINT_NO_UPDATE_CHECK === "1";
}

/**
 * Detect only source checkouts & GitHub Release/native-installer layouts.
 * Distribution through package-manager-owned channels is intentionally unsupported.
 */
export function detectInstallOwner() {
  const self = installationRoot();
  // GitHub Release archive / native installer: identifiable by our launcher layout.
  if (hasPortableLayout(self)) {
    return { owner: "portable", command: null, root: self };
  }
  return { owner: "source", command: null, root: self };
}

export function channelEnabled(channel, { offline = false } = {}) {
  if (offline) return false;
  if (updateChecksDisabled()) return false;
  return CHANNELS.includes(channel);
}
