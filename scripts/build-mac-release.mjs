// build-mac-release.mjs — never assembles/writes the release manifest.
// Bracket: writer pre-check -> tauri build -> release-assets check-built ->
// release-assets check-packaged -> writer pre-check -> notarytool/stapler/validate/spctl.

import { spawnSync } from "node:child_process";
import { existsSync, readFileSync } from "node:fs";
import { notarytoolAuthArgs } from "@rightkit/release/notary-auth.mjs";

const version = JSON.parse(readFileSync(new URL("../package.json", import.meta.url), "utf8")).version;
const dmg = `src-tauri/target/release/bundle/dmg/Membrane Hub_${version}_aarch64.dmg`;
const env = {
  ...process.env,
  APPLE_SIGNING_IDENTITY: process.env.APPLE_SIGNING_IDENTITY || "Developer ID Application: Adrian D'souza (6KLGD3LLKF)",
};

run("node", ["scripts/write-release-manifest.mjs", "check", "--require-committed"], env);
run("pnpm", ["exec", "tauri", "build", "--bundles", "app,dmg"], env);
run("node", ["scripts/release-assets.mjs", "check-built", "--platform", "mac"], env);
run("node", ["scripts/release-assets.mjs", "check-packaged", "--platform", "mac"], env);
if (!existsSync(dmg)) throw new Error(`missing signed DMG: ${dmg}`);
run("node", ["scripts/write-release-manifest.mjs", "check", "--require-committed"], env);
run("xcrun", ["notarytool", "submit", dmg, ...notarytoolAuthArgs(), "--wait"], env);
run("xcrun", ["stapler", "staple", dmg], env);
run("xcrun", ["stapler", "validate", dmg], env);
run("spctl", ["-a", "-vv", "--type", "open", "--context", "context:primary-signature", dmg], env);

function run(command, args, commandEnv) {
  const result = spawnSync(command, args, { stdio: "inherit", env: commandEnv });
  if (result.error) throw result.error;
  if (result.status !== 0) throw new Error(`${command} failed with exit ${result.status}`);
}
