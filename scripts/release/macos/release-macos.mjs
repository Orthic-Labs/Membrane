#!/usr/bin/env node
import { createHash } from "node:crypto";
import { existsSync, lstatSync, readFileSync, readdirSync, readlinkSync, writeFileSync } from "node:fs";
import { basename, dirname, relative, resolve, sep } from "node:path";
import { spawnSync } from "node:child_process";
import { fileURLToPath } from "node:url";

const root = resolve(dirname(fileURLToPath(import.meta.url)), "../../..");
const usage = "release-macos.mjs <verify|notarize|staple|receipt|verify-receipt> --app PATH --dmg PATH [options]";
const args = process.argv.slice(2);
const command = args.shift();
const value = (name) => { const i = args.indexOf(name); return i < 0 ? undefined : args[i + 1]; };
const fail = (message) => { console.error(`release-macos: ${message}`); process.exit(2); };
const sha256 = (path) => { const hash = createHash("sha256"); const walk = (entry) => { const stat = lstatSync(entry); hash.update(relative(path, entry)); if (stat.isSymbolicLink()) hash.update(readlinkSync(entry)); else if (stat.isDirectory()) for (const name of readdirSync(entry).sort()) walk(resolve(entry, name)); else hash.update(readFileSync(entry)); }; walk(path); return hash.digest("hex"); };
const confinedOutput = (path) => { const target = resolve(root, path); if (!target.startsWith(`${root}${sep}`)) fail("--out must stay inside repository"); return target; };
if (!command || !["verify", "notarize", "staple", "receipt", "verify-receipt"].includes(command)) fail(usage);
const app = value("--app"); const dmg = value("--dmg");
if (command === "verify-receipt") {
  const receiptPath = value("--receipt"); if (!receiptPath || !existsSync(resolve(root, receiptPath))) fail("--receipt must name an existing file");
  if (!app || !dmg) fail("--app and --dmg are required");
  if (!existsSync(resolve(root, app)) || !existsSync(resolve(root, dmg))) fail("receipt artifacts are missing");
  let receipt; try { receipt = JSON.parse(readFileSync(resolve(root, receiptPath), "utf8")); } catch { fail("receipt JSON invalid"); }
  if (receipt.schema !== "orthic.membrane.macos-release-receipt.v1" || receipt.publish !== false || !/^[0-9a-f]{40}$/.test(receipt.commit) || !/^\d+\.\d+\.\d+$/.test(receipt.version) || !["codesign", "notary", "staple"].every(key => receipt.checks?.[key] === "validated")) fail("receipt schema/checks invalid");
  if (receipt.appSha256 !== sha256(resolve(root, app)) || receipt.dmgSha256 !== sha256(resolve(root, dmg))) fail("receipt hash mismatch");
  process.stdout.write("macOS release receipt verified\n"); process.exit(0);
}
if (!app || !dmg) fail("--app and --dmg are required");
const appPath = resolve(root, app); const dmgPath = resolve(root, dmg);
if (!existsSync(appPath) || !appPath.endsWith(".app")) fail(`missing app or invalid .app: ${app}`);
if (!existsSync(dmgPath) || !dmgPath.endsWith(".dmg")) fail(`missing dmg or invalid .dmg: ${dmg}`);
const run = (tool, argv) => { const result = spawnSync(tool, argv, { stdio: "inherit" }); if (result.status !== 0) fail(`${tool} failed`); };
if (command === "verify") {
  run("codesign", ["--verify", "--deep", "--strict", "--verbose=2", appPath]);
  run("hdiutil", ["verify", dmgPath]);
  process.stdout.write("macOS signatures and DMG integrity verified\n");
} else if (command === "notarize") {
  const key = value("--keychain-profile"); if (!key) fail("--keychain-profile is required");
  run("xcrun", ["notarytool", "submit", dmgPath, "--keychain-profile", key, "--wait"]);
} else if (command === "staple") {
  run("xcrun", ["stapler", "staple", appPath]); run("xcrun", ["stapler", "staple", dmgPath]);
  run("xcrun", ["stapler", "validate", appPath]); run("xcrun", ["stapler", "validate", dmgPath]);
} else {
  const commit = value("--commit"); const version = value("--version"); const out = value("--out");
  if (!/^[0-9a-f]{40}$/.test(commit || "") || !/^\d+\.\d+\.\d+$/.test(version || "") || !out) fail("receipt requires valid --commit, --version, and --out");
  run("codesign", ["--verify", "--deep", "--strict", "--verbose=2", appPath]); run("hdiutil", ["verify", dmgPath]);
  run("xcrun", ["stapler", "validate", appPath]); run("xcrun", ["stapler", "validate", dmgPath]);
  const receipt = { schema: "orthic.membrane.macos-release-receipt.v1", product: "Membrane", commit, version,
    app: basename(appPath), dmg: basename(dmgPath), appSha256: sha256(appPath), dmgSha256: sha256(dmgPath),
    checks: { codesign: "validated", notary: "validated", staple: "validated" }, publish: false };
  writeFileSync(confinedOutput(out), `${JSON.stringify(receipt, null, 2)}\n`, { flag: "wx" });
  process.stdout.write(`receipt written: ${out}\n`);
}
