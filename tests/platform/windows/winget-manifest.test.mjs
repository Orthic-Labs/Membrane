import test from "node:test";
import assert from "node:assert/strict";
import { readFile } from "node:fs/promises";
import {
  PACKAGE_IDENTIFIER,
  DEFAULT_LOCALE,
  buildWingetManifests,
  unavailableWingetManifests,
  validateWingetManifests,
  renderWingetYaml,
} from "../../../scripts/release/channels/winget-manifest.mjs";

const READY_CONTRACT = {
  state: "ready",
  sourceIdentity: { commit: "a".repeat(40), version: "v1.2.3", releaseGeneration: "b".repeat(64) },
  artifact: { name: "Membrane_1.2.3_x64-setup.exe", url: "https://downloads.example/Membrane_1.2.3_x64-setup.exe", sha256: "c".repeat(64), type: "nsis", silentInstall: "/S", silentUninstall: "/S" },
  channels: {
    winget: { packageIdentifier: PACKAGE_IDENTIFIER, installerUrl: "https://downloads.example/Membrane_1.2.3_x64-setup.exe", installerSha256: "c".repeat(64) },
    scoop: { bucket: "orthic", url: "https://downloads.example/Membrane_1.2.3_x64-setup.exe", sha256: "c".repeat(64) },
  },
};

test("buildWingetManifests refuses a non-ready contract", () => {
  assert.throws(() => buildWingetManifests({ state: "unavailable" }), /not ready/);
});

test("buildWingetManifests binds one real signed x64 NSIS installer across all three files", () => {
  const { version, installer, locale } = buildWingetManifests(READY_CONTRACT);
  assert.equal(version.PackageIdentifier, PACKAGE_IDENTIFIER);
  assert.equal(version.PackageVersion, "1.2.3");
  assert.equal(version.DefaultLocale, DEFAULT_LOCALE);
  assert.equal(installer.PackageVersion, "1.2.3");
  assert.equal(installer.InstallerType, "nsis");
  assert.equal(installer.UpgradeBehavior, "install");
  assert.equal(installer.Installers.length, 1);
  assert.equal(installer.Installers[0].Architecture, "x64");
  assert.equal(installer.Installers[0].InstallerUrl, READY_CONTRACT.artifact.url);
  assert.equal(installer.Installers[0].InstallerSha256, READY_CONTRACT.artifact.sha256);
  assert.equal(installer.Installers[0].InstallerSwitches.Silent, "/S");
  assert.equal(installer.Installers[0].InstallerSwitches.SilentWithProgress, "/S");
  assert.equal(locale.PackageLocale, DEFAULT_LOCALE);
  assert.equal(locale.PackageVersion, "1.2.3");
  assert.doesNotThrow(() => validateWingetManifests({ version, installer, locale }));
});

test("unavailableWingetManifests is intentionally invalid, never a plausible fake", () => {
  const trio = unavailableWingetManifests();
  assert.equal(trio.version.PackageVersion, "0.0.0-unavailable");
  assert.match(trio.installer.Installers[0].InstallerUrl, /^UNAVAILABLE:/);
  assert.match(trio.installer.Installers[0].InstallerSha256, /^UNAVAILABLE-/);
  assert.throws(() => validateWingetManifests(trio), /SemVer|InstallerUrl|InstallerSha256/);
});

test("validateWingetManifests fails closed on an inconsistent or placeholder trio", () => {
  const { version, installer, locale } = buildWingetManifests(READY_CONTRACT);
  assert.throws(() => validateWingetManifests({ version: { ...version, PackageVersion: "1.2.3.0" }, installer, locale }), /SemVer/);
  assert.throws(() => validateWingetManifests({ version, installer: { ...installer, PackageVersion: "9.9.9" }, locale }), /match version manifest/);
  const brokenUrl = { ...installer, Installers: [{ ...installer.Installers[0], InstallerUrl: "not-a-url" }] };
  assert.throws(() => validateWingetManifests({ version, installer: brokenUrl, locale }), /InstallerUrl/);
  const brokenHash = { ...installer, Installers: [{ ...installer.Installers[0], InstallerSha256: "not-hex" }] };
  assert.throws(() => validateWingetManifests({ version, installer: brokenHash, locale }), /InstallerSha256/);
  const brokenSwitch = { ...installer, Installers: [{ ...installer.Installers[0], InstallerSwitches: { Silent: "/quiet", SilentWithProgress: "/S" } }] };
  assert.throws(() => validateWingetManifests({ version, installer: brokenSwitch, locale }), /silent/);
});

test("renderWingetYaml emits deterministic, correctly nested YAML", () => {
  const { installer } = buildWingetManifests(READY_CONTRACT);
  const text = renderWingetYaml(installer);
  assert.match(text, /^PackageIdentifier: "Orthic\.Membrane"$/m);
  assert.match(text, /^InstallModes:\n(?: {2}- "silent"\n {2}- "silentWithProgress"\n {2}- "interactive"\n)/m);
  assert.match(text, /^Installers:\n {2}- Architecture: "x64"\n {4}InstallerUrl: "https:\/\/downloads\.example\/Membrane_1\.2\.3_x64-setup\.exe"\n {4}InstallerSha256: "c{64}"\n {4}InstallerSwitches:\n {6}Silent: "\/S"\n {6}SilentWithProgress: "\/S"\n/m);
});

test("the committed UNAVAILABLE manifest trio matches the fail-closed generator output exactly", async () => {
  const dir = "packaging/winget/manifests/o/Orthic/Membrane/UNAVAILABLE";
  const trio = unavailableWingetManifests();
  const [version, installer, locale] = await Promise.all([
    readFile(`${dir}/${PACKAGE_IDENTIFIER}.yaml`, "utf8"),
    readFile(`${dir}/${PACKAGE_IDENTIFIER}.installer.yaml`, "utf8"),
    readFile(`${dir}/${PACKAGE_IDENTIFIER}.locale.en-US.yaml`, "utf8"),
  ]);
  assert.equal(version, renderWingetYaml(trio.version));
  assert.equal(installer, renderWingetYaml(trio.installer));
  assert.equal(locale, renderWingetYaml(trio.locale));
  assert.throws(() => validateWingetManifests(trio));
});
