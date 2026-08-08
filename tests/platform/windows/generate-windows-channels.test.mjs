import test from "node:test";
import assert from "node:assert/strict";
import { createHash } from "node:crypto";
import { mkdir, mkdtemp, readFile, writeFile } from "node:fs/promises";
import { tmpdir } from "node:os";
import { join } from "node:path";
import {
  PUBLIC_DOWNLOAD_BASE,
  sealedWindowsDir,
  loadWindowsChannelArtifact,
  verifySealedWindowsManifest,
  deriveWindowsChannelArtifact,
} from "../../../scripts/release/channels/right-release-source.mjs";
import { generateWindowsChannels, writeGeneratedChannels, CONTRACT_PATH, SCOOP_PATH } from "../../../scripts/release/channels/generate-windows-channels.mjs";

const APP = "membrane-hub";
const VERSION = "9.9.9";
const COMMIT = "1".repeat(40);
const INSTALLER_NAME = `Membrane_${VERSION}_x64-setup.exe`;
const INSTALLER_BYTES = Buffer.from("fake NSIS installer bytes for MBR-905 fixture only, never a real artifact");
const INSTALLER_SHA256 = createHash("sha256").update(INSTALLER_BYTES).digest("hex");
const ROUTE_KEY = "membrane/installers/windows/current/Membrane_x64-setup.exe";

function sealedManifest(overrides = {}) {
  return {
    schema: 1,
    releaseId: `${APP}-${VERSION}-${COMMIT.slice(0, 8)}`,
    app: APP,
    version: VERSION,
    commit: COMMIT,
    platform: "win",
    cacheKey: "deadbeefcafef00d",
    files: [{ role: "installer", name: INSTALLER_NAME, sha256: INSTALLER_SHA256, sizeBytes: INSTALLER_BYTES.length }],
    routes: { patch: [{ role: "installer", name: INSTALLER_NAME, bucket: "public", key: ROUTE_KEY }], update: [] },
    checkpoints: ["preflight_complete", "build_complete", "signed", "hardened", "sealed"],
    sealedAt: new Date().toISOString(),
    ...overrides,
  };
}

async function seedSealedRelease(root, manifestOverrides = {}, { skipInstallerFile = false } = {}) {
  const dir = sealedWindowsDir(root, APP, VERSION, COMMIT);
  await mkdir(dir, { recursive: true });
  if (!skipInstallerFile) await writeFile(join(dir, INSTALLER_NAME), INSTALLER_BYTES);
  const manifest = sealedManifest(manifestOverrides);
  await writeFile(join(dir, "release-manifest.json"), JSON.stringify(manifest));
  return { dir, manifest };
}

async function seedRoot() {
  const root = await mkdtemp(join(tmpdir(), "membrane-windows-channels-"));
  await mkdir(join(root, "packaging", "winget"), { recursive: true });
  await writeFile(join(root, CONTRACT_PATH), await readFile("packaging/winget/release.v1.json", "utf8"));
  return root;
}

function passingReceipt() {
  return { schema: "windows-installer-receipt.v1", status: "pass", source_commit: COMMIT, installer_sha256: INSTALLER_SHA256, gates: { signature: "pass", install: "pass", update: "pass", uninstall: "pass" } };
}

// --- right-release-source.mjs -------------------------------------------

test("today's real membrane checkout has no sealed Windows release: nothing is invented", async () => {
  const artifact = loadWindowsChannelArtifact({ root: process.cwd(), app: APP, version: "0.1.5", commit: "0".repeat(40) });
  assert.equal(artifact, null);
});

test("verifySealedWindowsManifest fails closed on a tampered installer, a missing checkpoint, or a bad commit", async () => {
  const root = await mkdtemp(join(tmpdir(), "membrane-sealed-"));
  const { dir } = await seedSealedRelease(root);
  const manifest = JSON.parse(await readFile(join(dir, "release-manifest.json"), "utf8"));
  assert.doesNotThrow(() => verifySealedWindowsManifest(manifest, dir));

  await writeFile(join(dir, INSTALLER_NAME), Buffer.from("tampered bytes"));
  assert.throws(() => verifySealedWindowsManifest(manifest, dir), /hash mismatch/);

  await writeFile(join(dir, INSTALLER_NAME), INSTALLER_BYTES);
  const noHardening = { ...manifest, checkpoints: manifest.checkpoints.filter((c) => c !== "hardened") };
  assert.throws(() => verifySealedWindowsManifest(noHardening, dir), /missing checkpoint hardened/);

  const badCommit = { ...manifest, commit: "not-a-commit" };
  assert.throws(() => verifySealedWindowsManifest(badCommit, dir), /commit must be a 40-character/);
});

test("deriveWindowsChannelArtifact declares a gap instead of guessing on multiple or non-x64 installers", async () => {
  const root = await mkdtemp(join(tmpdir(), "membrane-sealed-gap-"));
  const { dir } = await seedSealedRelease(root, {
    files: [
      { role: "installer", name: INSTALLER_NAME, sha256: INSTALLER_SHA256, sizeBytes: INSTALLER_BYTES.length },
      { role: "installer", name: `Membrane_${VERSION}_arm64-setup.exe`, sha256: "0".repeat(64), sizeBytes: 1 },
    ],
  });
  const manifest = JSON.parse(await readFile(join(dir, "release-manifest.json"), "utf8"));
  // deriveWindowsChannelArtifact only inspects manifest structure (the
  // architecture-count gap), so this does not need the second, unused
  // fixture file's bytes to hash-verify.
  assert.throws(() => deriveWindowsChannelArtifact(manifest), /declared gap.*2 installer artifacts/s);
});

test("deriveWindowsChannelArtifact binds the exact RightKit-sourced hash and the real public R2 URL", async () => {
  const root = await mkdtemp(join(tmpdir(), "membrane-sealed-derive-"));
  const { dir } = await seedSealedRelease(root);
  const manifest = JSON.parse(await readFile(join(dir, "release-manifest.json"), "utf8"));
  verifySealedWindowsManifest(manifest, dir);
  const artifact = deriveWindowsChannelArtifact(manifest);
  assert.equal(artifact.version, VERSION);
  assert.equal(artifact.commit, COMMIT);
  assert.equal(artifact.installerName, INSTALLER_NAME);
  assert.equal(artifact.installerSha256, INSTALLER_SHA256);
  assert.equal(artifact.installerUrl, `${PUBLIC_DOWNLOAD_BASE}/${ROUTE_KEY}`);
});

// --- generate-windows-channels.mjs --------------------------------------

test("generateWindowsChannels reports unavailable, and writes nothing, when no sealed release or receipt exists", async () => {
  const root = await seedRoot();
  const result = await generateWindowsChannels({ root, version: VERSION, commit: COMMIT });
  assert.equal(result.status, "unavailable");
  assert.ok(result.reasons.some((r) => r.includes("no sealed RightKit Windows release found")));
  assert.ok(result.reasons.some((r) => r.includes("no MBR-902 clean-machine receipt supplied")));
});

test("generateWindowsChannels turns a verified sealed release plus a passing receipt into ready, validated Winget & Scoop manifests", async () => {
  const root = await seedRoot();
  await seedSealedRelease(root);
  const receiptPath = "evidence/receipt.json";
  await mkdir(join(root, "evidence"), { recursive: true });
  await writeFile(join(root, receiptPath), JSON.stringify(passingReceipt()));

  const result = await generateWindowsChannels({ root, version: VERSION, commit: COMMIT, receiptPath });
  assert.equal(result.status, "ready");
  assert.equal(result.contract.artifact.sha256, INSTALLER_SHA256);
  assert.equal(result.contract.artifact.url, `${PUBLIC_DOWNLOAD_BASE}/${ROUTE_KEY}`);
  assert.equal(result.contract.channels.winget.installerSha256, INSTALLER_SHA256);
  assert.equal(result.contract.channels.scoop.sha256, INSTALLER_SHA256);
  assert.equal(result.winget.installer.Installers[0].InstallerUrl, result.contract.artifact.url);
  assert.equal(result.scoop.hash, `sha256:${INSTALLER_SHA256}`);

  await writeGeneratedChannels(root, result);
  const manifestDir = join(root, "packaging", "winget", "manifests", "o", "Orthic", "Membrane", VERSION);
  const [versionYaml, installerYaml, localeYaml, contractOnDisk, scoopOnDisk] = await Promise.all([
    readFile(join(manifestDir, "Orthic.Membrane.yaml"), "utf8"),
    readFile(join(manifestDir, "Orthic.Membrane.installer.yaml"), "utf8"),
    readFile(join(manifestDir, "Orthic.Membrane.locale.en-US.yaml"), "utf8"),
    readFile(join(root, CONTRACT_PATH), "utf8"),
    readFile(join(root, SCOOP_PATH), "utf8"),
  ]);
  assert.match(installerYaml, new RegExp(INSTALLER_SHA256));
  assert.match(versionYaml, /PackageVersion: "9\.9\.9"/);
  assert.match(localeYaml, /PackageLocale: "en-US"/);
  assert.equal(JSON.parse(contractOnDisk).state, "ready");
  assert.equal(JSON.parse(scoopOnDisk).hash, `sha256:${INSTALLER_SHA256}`);
});

test("generateWindowsChannels stays unavailable when the receipt does not match the sealed hash", async () => {
  const root = await seedRoot();
  await seedSealedRelease(root);
  const receiptPath = "evidence/receipt.json";
  await mkdir(join(root, "evidence"), { recursive: true });
  await writeFile(join(root, receiptPath), JSON.stringify({ ...passingReceipt(), installer_sha256: "0".repeat(64) }));

  const result = await generateWindowsChannels({ root, version: VERSION, commit: COMMIT, receiptPath });
  assert.equal(result.status, "unavailable");
  assert.ok(result.reasons.some((r) => /receipt identity does not match manifest/.test(r)));
});
