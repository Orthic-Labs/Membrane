import { spawnSync } from "node:child_process";
import { existsSync, readFileSync } from "node:fs";
import { dirname, join, resolve } from "node:path";
import { fileURLToPath } from "node:url";
import { notarytoolAuthArgs } from "@rightkit/release/notary-auth.mjs";
import { cargoTargetRoot } from "./lib/target-root.mjs";

const appRoot = resolve(dirname(fileURLToPath(import.meta.url)), "..");
const MAC_TARGET = "aarch64-apple-darwin";
const version = JSON.parse(readFileSync(new URL("../package.json", import.meta.url), "utf8")).version;
// Resolve through cargo (honouring a managed build's CARGO_TARGET_DIR) rather
// than assuming src-tauri/target — a managed build writes elsewhere.
const dmg = join(cargoTargetRoot(appRoot), MAC_TARGET, "release", "bundle", "dmg", `Orthic_${version}_aarch64.dmg`);
const env = {
  ...process.env,
  APPLE_SIGNING_IDENTITY: process.env.APPLE_SIGNING_IDENTITY || "Developer ID Application: Adrian D'souza (6KLGD3LLKF)",
};

run("node", ["scripts/stage-binaries.mjs"], env);
run("pnpm", ["exec", "tauri", "build", "--release", "--target", MAC_TARGET, "--bundles", "app,dmg"], env);
if (!existsSync(dmg)) throw new Error(`missing signed DMG: ${dmg}`);
run("xcrun", ["notarytool", "submit", dmg, ...notarytoolAuthArgs(), "--wait"], env);
run("xcrun", ["stapler", "staple", dmg], env);
run("xcrun", ["stapler", "validate", dmg], env);
run("spctl", ["-a", "-vv", "--type", "open", "--context", "context:primary-signature", dmg], env);

function run(command, args, commandEnv) {
  const result = spawnSync(command, args, { stdio: "inherit", env: commandEnv });
  if (result.error) throw result.error;
  if (result.status !== 0) throw new Error(`${command} failed with exit ${result.status}`);
}
