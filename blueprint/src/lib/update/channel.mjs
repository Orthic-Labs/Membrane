// D16: update channels and install-owner detection. Package-manager channels
// print the exact owning-manager update command and never self-replace.
// Portable/native channels require a signed manifest and matching checksum.

import { existsSync } from "node:fs";
import { homedir } from "node:os";
import { dirname, join, resolve } from "node:path";
import { fileURLToPath } from "node:url";

export const CHANNELS = Object.freeze(["stable", "beta", "nightly"]);
const SCRIPT_DIR = dirname(fileURLToPath(import.meta.url));

export function detectInstallOwner() {
  const self = resolve(SCRIPT_DIR, "..", "..", "..");
  // npm/pnpm global install: the package lives under a node_modules tree.
  if (self.includes(`${sep()}node_modules${sep()}`)) {
    const parts = self.split(sep());
    const nmIndex = parts.lastIndexOf("node_modules");
    const globalRoot = parts.slice(0, nmIndex).join(sep());
    const pkg = parts[nmIndex + 1];
    return { owner: "npm", command: `npm update -g ${pkg}`, root: globalRoot };
  }
  // Homebrew: /opt/homebrew or /usr/local Cellar paths.
  if (/Cellar|cortex\/(\d+\.\d+)/.test(self) && existsSync(join(self, "..", "..", "..", "Homebrew"))) {
    return { owner: "homebrew", command: "brew upgrade cortex", root: self };
  }
  // WinGet: %LOCALAPPDATA%\Microsoft\WinGet\Packages
  if (process.platform === "win32" && self.includes("WinGet")) {
    return { owner: "winget", command: "winget upgrade OrthicLabs.Cortex", root: self };
  }
  // Portable archive / native installer: identifiable by our own launcher layout.
  if (existsSync(join(self, "lib", process.platform === "win32" ? "node.exe" : "node")) && existsSync(join(self, "app", "package"))) {
    return { owner: "portable", command: null, root: self };
  }
  return { owner: "source", command: null, root: self };
}

export function channelEnabled(channel, { offline = false } = {}) {
  if (offline) return false;
  if (process.env.CORTEX_NO_UPDATE_CHECK === "1") return false;
  return CHANNELS.includes(channel);
}

function sep() {
  return process.platform === "win32" ? "\\" : "/";
}
