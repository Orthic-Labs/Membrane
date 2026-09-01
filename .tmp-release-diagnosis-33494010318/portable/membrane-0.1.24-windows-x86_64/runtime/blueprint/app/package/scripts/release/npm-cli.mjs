import { execFileSync } from "node:child_process";
import { existsSync, readFileSync } from "node:fs";
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

export function pnpmCliArgs(args) {
  if (process.platform !== "win32") throw new Error("pnpm CLI discovery is Windows-only");
  const wrapper = execFileSync("where.exe", ["pnpm.CMD"], { encoding: "utf8" })
    .split(/\r?\n/)
    .map((line) => line.trim())
    .find(Boolean);
  if (!wrapper || !existsSync(wrapper)) throw new Error("Provisioned pnpm.CMD not found");
  const match = readFileSync(wrapper, "utf8").match(/"%~?dp0%?\\([^"\r\n]*\\pnpm\.mjs)"/i);
  if (!match) throw new Error(`Provisioned pnpm CLI path not found in ${wrapper}`);
  const cli = resolve(dirname(wrapper), match[1]);
  if (!existsSync(cli)) throw new Error(`Provisioned pnpm CLI not found: ${cli}`);
  return [cli, ...args];
}
