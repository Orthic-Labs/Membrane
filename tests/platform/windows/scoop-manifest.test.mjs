import test from "node:test";
import assert from "node:assert/strict";
import { readFile } from "node:fs/promises";
import { buildScoopManifest, unavailableScoopManifest, validateScoopManifest } from "../../../scripts/release/channels/scoop-manifest.mjs";

const READY_CONTRACT = {
  state: "ready",
  sourceIdentity: { commit: "a".repeat(40), version: "v1.2.3", releaseGeneration: "b".repeat(64) },
  artifact: { name: "Membrane_1.2.3_x64-setup.exe", url: "https://downloads.example/Membrane_1.2.3_x64-setup.exe", sha256: "c".repeat(64), type: "nsis", silentInstall: "/S", silentUninstall: "/S" },
  channels: {
    winget: { packageIdentifier: "Orthic.Membrane", installerUrl: "https://downloads.example/Membrane_1.2.3_x64-setup.exe", installerSha256: "c".repeat(64) },
    scoop: { bucket: "orthic", url: "https://downloads.example/Membrane_1.2.3_x64-setup.exe", sha256: "c".repeat(64) },
  },
};

test("buildScoopManifest refuses a non-ready contract", () => {
  assert.throws(() => buildScoopManifest({ state: "unavailable" }), /not ready/);
});

test("buildScoopManifest binds the same signed x64 NSIS installer Winget uses, with silent install/uninstall and version-detected upgrade", () => {
  const manifest = buildScoopManifest(READY_CONTRACT);
  assert.equal(manifest.version, "1.2.3");
  assert.equal(manifest.url, READY_CONTRACT.channels.scoop.url);
  assert.equal(manifest.hash, `sha256:${READY_CONTRACT.channels.scoop.sha256}`);
  assert.equal(manifest.architecture["64bit"].installer.file, READY_CONTRACT.artifact.name);
  assert.deepEqual(manifest.architecture["64bit"].installer.args, ["/S"]);
  assert.deepEqual(manifest.architecture["64bit"].uninstaller.args, ["/S"]);
  assert.match(manifest.checkver.url, /^https:\/\//);
  assert.match(manifest.autoupdate.architecture["64bit"].url, /\$version/);
  assert.doesNotThrow(() => validateScoopManifest(manifest));
});

test("unavailableScoopManifest matches the committed packaging/scoop/membrane.json and is accepted as a non-installable placeholder", async () => {
  const committed = JSON.parse(await readFile("packaging/scoop/membrane.json", "utf8"));
  assert.deepEqual(committed, unavailableScoopManifest());
  assert.equal("url" in committed, false);
  assert.equal("hash" in committed, false);
  assert.doesNotThrow(() => validateScoopManifest(committed));
});

test("validateScoopManifest fails closed on a placeholder or half-specified installable manifest", () => {
  const manifest = buildScoopManifest(READY_CONTRACT);
  assert.throws(() => validateScoopManifest({ ...manifest, url: "not-a-url" }), /url/);
  assert.throws(() => validateScoopManifest({ ...manifest, hash: "not-a-hash" }), /hash/);
  const noSilentInstall = { ...manifest, architecture: { "64bit": { ...manifest.architecture["64bit"], installer: { file: manifest.artifact ?? manifest.architecture["64bit"].installer.file, args: [] } } } };
  assert.throws(() => validateScoopManifest(noSilentInstall), /installer must run with silent flag/);
  const noSilentUninstall = { ...manifest, architecture: { "64bit": { ...manifest.architecture["64bit"], uninstaller: { file: "uninstall.exe", args: [] } } } };
  assert.throws(() => validateScoopManifest(noSilentUninstall), /uninstaller must run with silent flag/);
  const noAutoupdate = { ...manifest, autoupdate: { architecture: { "64bit": { url: "https://static.example/no-version-token" } }, hash: { url: "$url.sha256" } } };
  assert.throws(() => validateScoopManifest(noAutoupdate), /\$version/);
  assert.throws(() => validateScoopManifest({ version: "1.2.3", hash: `sha256:${"c".repeat(64)}` }), /unavailable manifest must not declare a hash without a url/);
});
