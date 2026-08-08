#!/usr/bin/env node
import { createHash } from "node:crypto";
import { lstat, readFile, realpath } from "node:fs/promises";
import { isAbsolute, relative, resolve, sep } from "node:path";
import { validateReceipt } from "./windows/contract.mjs";

const fail = message => { throw new Error(message); };
const closed = (value, keys, label) => {
  if (!value || typeof value !== "object" || Array.isArray(value) || Object.keys(value).some(key => !keys.includes(key))) fail(`${label} invalid`);
};
const commit = value => typeof value === "string" && /^[0-9a-f]{40}$/.test(value);
const digest = value => typeof value === "string" && /^[0-9a-f]{64}$/.test(value);
const version = value => typeof value === "string" && /^v?\d+\.\d+\.\d+$/.test(value);
const url = value => { try { const parsed = new URL(value); return parsed.protocol === "https:" && !parsed.username && !parsed.password; } catch { return false; } };
const evidence = async (root, ref) => {
  closed(ref, ["path", "sha256"], "trust receipt");
  if (isAbsolute(ref.path) || relative(root, resolve(root, ref.path)).startsWith(`..${sep}`) || !digest(ref.sha256)) fail("trust receipt invalid");
  const candidate = resolve(root, ref.path); const [rootReal, fileReal, stat] = await Promise.all([realpath(root), realpath(candidate), lstat(candidate)]);
  if (stat.isSymbolicLink() || !stat.isFile() || !fileReal.startsWith(`${rootReal}${sep}`)) fail("trust receipt invalid");
  const bytes = await readFile(candidate); if (createHash("sha256").update(bytes).digest("hex") !== ref.sha256) fail("trust receipt hash mismatch");
  return JSON.parse(bytes);
};

export async function verifyWindowsChannels(value, root = process.cwd()) {
  closed(value, ["schema", "state", "publish", "sourceIdentity", "artifact", "trust", "upgrade", "channels"], "contract");
  if (value.schema !== "orthic.membrane.windows-channels.v1" || value.publish !== false) fail("contract schema or publish invalid");
  closed(value.sourceIdentity, ["commit", "version", "releaseGeneration"], "source identity");
  closed(value.artifact, ["name", "url", "sha256", "type", "silentInstall", "silentUninstall"], "artifact");
  closed(value.trust, ["receipt"], "trust");
  closed(value.upgrade, ["detectInstalledVersion", "allowDowngrade"], "upgrade");
  closed(value.channels, ["winget", "scoop"], "channels");
  closed(value.channels.winget, ["packageIdentifier", "installerUrl", "installerSha256"], "winget");
  closed(value.channels.scoop, ["bucket", "url", "sha256"], "scoop");
  if (value.state === "unavailable") {
    const unavailable = [value.sourceIdentity.commit, value.sourceIdentity.version, value.sourceIdentity.releaseGeneration, value.artifact.name, value.artifact.url, value.artifact.sha256, value.trust.receipt, value.channels.winget.installerUrl, value.channels.winget.installerSha256, value.channels.scoop.url, value.channels.scoop.sha256];
    if (!unavailable.every(item => item === null)) fail("unavailable channel must contain no installable placeholder");
    return { status: "unavailable" };
  }
  if (value.state !== "ready") fail("channel state invalid");
  if (!commit(value.sourceIdentity.commit) || !version(value.sourceIdentity.version) || !digest(value.sourceIdentity.releaseGeneration)) fail("source identity invalid");
  if (value.artifact.name !== `Membrane_${value.sourceIdentity.version.replace(/^v/, "")}_x64-setup.exe` || !url(value.artifact.url) || !digest(value.artifact.sha256) || value.artifact.type !== "nsis") fail("NSIS artifact invalid");
  if (value.artifact.silentInstall !== "/S" || value.artifact.silentUninstall !== "/S") fail("silent NSIS flags invalid");
  const manifest = { product_version: value.sourceIdentity.version.replace(/^v/, ""), source_commit: value.sourceIdentity.commit, installer_path: value.artifact.name, installer_sha256: value.artifact.sha256, signing: { provider: "azure-artifact-signing", trust: "public", timestamp: "rfc3161" } };
  validateReceipt(await evidence(root, value.trust.receipt), manifest);
  if (value.upgrade.detectInstalledVersion !== true || value.upgrade.allowDowngrade !== false) fail("upgrade detection invalid");
  if (value.channels.winget.packageIdentifier !== "Orthic.Membrane" || value.channels.winget.installerUrl !== value.artifact.url || value.channels.winget.installerSha256 !== value.artifact.sha256) fail("Winget artifact binding invalid");
  if (value.channels.scoop.bucket !== "orthic" || value.channels.scoop.url !== value.artifact.url || value.channels.scoop.sha256 !== value.artifact.sha256) fail("Scoop artifact binding invalid");
  return { status: "ready", version: value.sourceIdentity.version, artifactSha256: value.artifact.sha256 };
}

if (import.meta.url === `file://${process.argv[1]}`) {
  const [path] = process.argv.slice(2);
  if (!path) fail("usage: verify-windows-channels.mjs CONTRACT.json");
  process.stdout.write(`${JSON.stringify(await verifyWindowsChannels(JSON.parse(await readFile(path, "utf8"))))}\n`);
}
