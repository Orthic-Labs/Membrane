// D16: GitHub-release update channels and install-owner detection.
// Installed archives require a signed manifest and matching checksum.

import { existsSync } from "node:fs";
import { dirname, join, resolve } from "node:path";
import { fileURLToPath } from "node:url";

export const CHANNELS = Object.freeze(["stable", "beta", "nightly"]);
const SCRIPT_DIR = dirname(fileURLToPath(import.meta.url));

export function detectInstallOwner() {
  const self = resolve(SCRIPT_DIR, "..", "..", "..");
  // GitHub Release archive / native installer: identifiable by our launcher layout.
  if (existsSync(join(self, "lib", "node")) && existsSync(join(self, "app", "package"))) {
    return { owner: "portable", command: null, root: self };
  }
  return { owner: "source", command: null, root: self };
}

export function channelEnabled(channel, { offline = false } = {}) {
  if (offline) return false;
  if (process.env.BLUEPRINT_NO_UPDATE_CHECK === "1") return false;
  return CHANNELS.includes(channel);
}
