import { existsSync } from "node:fs";
import { dirname, resolve } from "node:path";

export function npmCliArgs(args) {
  const runtimeDir = dirname(process.execPath);
  const candidates = process.platform === "win32"
    ? [resolve(runtimeDir, "node_modules/npm/bin/npm-cli.js")]
    : [
        resolve(runtimeDir, "../lib/node_modules/npm/bin/npm-cli.js"),
        resolve(runtimeDir, "node_modules/npm/bin/npm-cli.js"),
      ];
  const cli = candidates.find(existsSync);
  if (!cli) throw new Error(`Bundled npm CLI not found for ${process.execPath}`);
  return [cli, ...args];
}
