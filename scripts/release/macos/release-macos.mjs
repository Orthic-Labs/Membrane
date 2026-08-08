#!/usr/bin/env node
// macOS release ACCEPTANCE layer.
//
// The canonical build/sign/notarize/harden/seal path is RightKit:
//   pnpm release:doctor          (right-release doctor --platform mac)
//   pnpm release:build:mac       (right-release build --platform mac)
// See docs/release/macos.md and apps/membrane-hub/right-release.config.mjs.
// That command builds, codesigns every embedded binary and the outer .app
// inner-to-outer with hardened runtime, notarizes, staples and Gatekeeper-
// assesses the DMG, hardening-scans the result, and seals a
// release-manifest.json binding every artifact's sha256. This script never
// re-implements any of that. It only re-verifies RightKit's real output
// (read-only checks: codesign --verify, hardened-runtime flag, spctl
// assessment, hdiutil integrity, stapler validate) and captures the two
// pieces of durable evidence RightKit's own build does not persist: a
// notarization log, and this task's acceptance receipts.
import { createHash } from "node:crypto";
import { existsSync, lstatSync, readFileSync, readdirSync, readlinkSync, writeFileSync } from "node:fs";
import { basename, dirname, relative, resolve, sep } from "node:path";
import { spawnSync } from "node:child_process";
import { fileURLToPath } from "node:url";
import {
  readJson, buildPlatformAcceptanceReceipt,
  readSealedReleaseManifest, verifySealedInstallerHash,
  notarytoolAuthArgs, NOTARY_KEYCHAIN_PROFILE_DEFAULT,
} from "./contract.mjs";

const root = resolve(dirname(fileURLToPath(import.meta.url)), "../../..");
const COMMANDS = ["verify", "receipt", "verify-receipt", "notarization-log", "platform-receipt"];
const usage = `release-macos.mjs <${COMMANDS.join("|")}> [options]`;
const args = process.argv.slice(2);
const command = args.shift();
const value = (name) => { const i = args.indexOf(name); return i < 0 ? undefined : args[i + 1]; };
const flag = (name) => args.includes(name);
const fail = (message) => { console.error(`release-macos: ${message}`); process.exit(2); };
const sha256 = (targetPath) => {
  const hash = createHash("sha256");
  const walk = (entry) => {
    const stat = lstatSync(entry);
    hash.update(relative(targetPath, entry));
    if (stat.isSymbolicLink()) hash.update(readlinkSync(entry));
    else if (stat.isDirectory()) for (const name of readdirSync(entry).sort()) walk(resolve(entry, name));
    else hash.update(readFileSync(entry));
  };
  walk(targetPath);
  return hash.digest("hex");
};
const confinedOutput = (outPath) => {
  const target = resolve(root, outPath);
  if (!target.startsWith(`${root}${sep}`)) fail("--out must stay inside repository");
  return target;
};
if (!command || !COMMANDS.includes(command)) fail(usage);

// --- commands that never shell out to a signing/notarization tool ---------

if (command === "notarization-log") {
  const submissionId = value("--submission-id");
  const profile = value("--keychain-profile") ?? NOTARY_KEYCHAIN_PROFILE_DEFAULT;
  const out = value("--out");
  if (!submissionId || !/^[0-9a-f]{8}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{12}$/i.test(submissionId)) {
    fail("--submission-id must be a notarytool UUID (see: xcrun notarytool history --keychain-profile <profile>)");
  }
  if (!out) fail("--out is required");
  let authArgs;
  try { authArgs = notarytoolAuthArgs({ profile }); }
  catch (error) { fail(error.message); }
  const result = spawnSync("xcrun", ["notarytool", "log", submissionId, ...authArgs], { encoding: "utf8" });
  if (result.status !== 0) fail(`xcrun notarytool log failed: ${result.stderr || result.stdout || "unknown error"}`);
  writeFileSync(confinedOutput(out), result.stdout, { flag: "wx" });
  process.stdout.write(`notarization log written: ${out}\n`); process.exit(0);
}
if (command === "platform-receipt") {
  const out = value("--out");
  if (!out) fail("--out is required");
  const input = {
    receiptId: value("--receipt-id"), mode: value("--mode"), commit: value("--commit"),
    releaseGeneration: value("--release-generation"), version: value("--version"),
    artifactName: value("--artifact-name"), artifactSha256: value("--artifact-sha256"),
    codesign: value("--codesign"), notarization: value("--notarization"), staple: value("--staple"), gatekeeper: value("--gatekeeper"),
    install: value("--install"), startup: value("--startup"), update: value("--update"), uninstall: value("--uninstall"),
    clean: flag("--clean"), machineDigest: value("--machine-digest"), bypassWarnings: flag("--bypass-warnings"),
  };
  let receipt;
  try { receipt = buildPlatformAcceptanceReceipt(input); }
  catch (error) { fail(`platform receipt invalid: ${error.message}`); }
  writeFileSync(confinedOutput(out), `${JSON.stringify(receipt, null, 2)}\n`, { flag: "wx" });
  process.stdout.write(`platform acceptance receipt written: ${out}\n`); process.exit(0);
}
if (command === "verify-receipt") {
  const app = value("--app"); const dmg = value("--dmg");
  const receiptPath = value("--receipt"); if (!receiptPath || !existsSync(resolve(root, receiptPath))) fail("--receipt must name an existing file");
  if (!app || !dmg) fail("--app and --dmg are required");
  if (!existsSync(resolve(root, app)) || !existsSync(resolve(root, dmg))) fail("receipt artifacts are missing");
  const receipt = await readJson(resolve(root, receiptPath)).catch(() => fail("receipt JSON invalid"));
  if (receipt.schema !== "orthic.membrane.macos-release-receipt.v1" || receipt.publish !== false || !/^[0-9a-f]{40}$/.test(receipt.commit) || !/^\d+\.\d+\.\d+$/.test(receipt.version) || !["codesign", "notary", "staple", "gatekeeper"].every(key => receipt.checks?.[key] === "validated")) fail("receipt schema/checks invalid");
  if (receipt.appSha256 !== sha256(resolve(root, app)) || receipt.dmgSha256 !== sha256(resolve(root, dmg))) fail("receipt hash mismatch");
  process.stdout.write("macOS release receipt verified\n"); process.exit(0);
}

// --- commands that operate on a built --app/--dmg pair (RightKit's own output) ---
const app = value("--app"); const dmg = value("--dmg");
if (!app || !dmg) fail("--app and --dmg are required");
const appPath = resolve(root, app); const dmgPath = resolve(root, dmg);
if (!existsSync(appPath) || !appPath.endsWith(".app")) fail(`missing app or invalid .app: ${app}`);
if (!existsSync(dmgPath) || !dmgPath.endsWith(".dmg")) fail(`missing dmg or invalid .dmg: ${dmg}`);

// Optional: cross-check --dmg is the exact byte-for-byte artifact RightKit
// sealed for this commit/version, not merely a same-named file elsewhere.
const sealedDirArg = value("--sealed-dir");
if (sealedDirArg) {
  const sealedDir = resolve(root, sealedDirArg);
  let sealed;
  try { sealed = readSealedReleaseManifest(sealedDir); }
  catch (error) { fail(error.message); }
  let sealedDmgPath;
  try { sealedDmgPath = verifySealedInstallerHash({ sealedDir, installer: sealed.installer }); }
  catch (error) { fail(error.message); }
  if (basename(sealedDmgPath) !== basename(dmgPath) || sha256(dmgPath) !== sealed.installer.sha256) {
    fail(`--dmg does not match RightKit's sealed installer: ${sealed.manifestPath}`);
  }
}

const run = (tool, argv) => { const result = spawnSync(tool, argv, { stdio: "inherit" }); if (result.status !== 0) fail(`${tool} failed`); };
const runCapture = (tool, argv) => { const result = spawnSync(tool, argv, { encoding: "utf8" }); if (result.status !== 0) fail(`${tool} failed: ${result.stderr || result.stdout || "unknown error"}`); return `${result.stdout}${result.stderr}`; };
const assertHardenedRuntime = (target) => { const display = runCapture("codesign", ["--display", "--verbose=4", target]); if (!/flags=0x[0-9a-f]*10000\(runtime\)/i.test(display)) fail(`hardened runtime is not enabled on ${target}`); };

// Read-only acceptance checks over what RightKit already produced. None of
// these mutate, sign, notarize, or staple anything — `stapler validate`
// checks an existing ticket, it does not staple one.
function runAcceptanceChecks() {
  run("codesign", ["--verify", "--deep", "--strict", "--verbose=2", appPath]);
  assertHardenedRuntime(appPath);
  run("spctl", ["--assess", "--type", "execute", "--verbose=2", appPath]);
  run("hdiutil", ["verify", dmgPath]);
  run("spctl", ["--assess", "--type", "open", "--context", "context:primary-signature", "--verbose=2", dmgPath]);
  run("xcrun", ["stapler", "validate", appPath]);
  run("xcrun", ["stapler", "validate", dmgPath]);
}

if (command === "verify") {
  runAcceptanceChecks();
  process.stdout.write("macOS signatures, hardened runtime, Gatekeeper assessment, DMG integrity, and staple validation verified\n");
} else if (command === "receipt") {
  const commit = value("--commit"); const version = value("--version"); const out = value("--out");
  if (!/^[0-9a-f]{40}$/.test(commit || "") || !/^\d+\.\d+\.\d+$/.test(version || "") || !out) fail("receipt requires valid --commit, --version, and --out");
  runAcceptanceChecks();
  const receipt = { schema: "orthic.membrane.macos-release-receipt.v1", product: "Membrane", commit, version,
    app: basename(appPath), dmg: basename(dmgPath), appSha256: sha256(appPath), dmgSha256: sha256(dmgPath),
    checks: { codesign: "validated", notary: "validated", staple: "validated", gatekeeper: "validated" }, publish: false };
  writeFileSync(confinedOutput(out), `${JSON.stringify(receipt, null, 2)}\n`, { flag: "wx" });
  process.stdout.write(`receipt written: ${out}\n`);
}
