import assert from "node:assert/strict";
import { readFileSync } from "node:fs";
import test from "node:test";

const pkg = JSON.parse(readFileSync(new URL("../package.json", import.meta.url), "utf8"));
const release = readFileSync(new URL("../scripts/release-build-portable-windows.mjs", import.meta.url), "utf8");
const packager = readFileSync(new URL("../scripts/package-portable-windows.mjs", import.meta.url), "utf8");
const finalizer = readFileSync(new URL("../scripts/finalize-portable-release.mjs", import.meta.url), "utf8");
const publisher = readFileSync(new URL("../scripts/publish-portable-release.mjs", import.meta.url), "utf8");

test("portable Windows lane signs raw app & archives without NSIS", () => {
  assert.equal(pkg.scripts["release:build:portable:win"], "node scripts/release-build-portable-windows.mjs");
  assert.match(release, /release:prepare:sidecars:win/);
  assert.match(release, /right-release", "sign-windows"/);
  assert.match(release, /rightkit:package:win", "--", "raw"/);
  assert.match(release, /materializeHardeningEvidence/);
  assert.match(release, /--allow-evidence/);
  assert.match(release, /runRightReleaseAtRepoRoot\(\["hardening"/);
  assert.match(release, /root: repoRoot/);
  assert.match(release, /engine\/crates\/membrane-adapt\/src\/remediation\.rs:74/);
  assert.match(release, /package-portable-windows\.mjs/);
  assert.match(release, /finalize-portable-release\.mjs/);
  assert.doesNotMatch(release, /right-release", "build"|rightkit:package:win", "--", "package"/i);
});

test("portable payload is signed, hashed & includes activation plus Agent Plugins core", () => {
  for (const name of ["membrane-hub.exe", "cortex.exe", "membrane.exe", "membrane-tray.exe", "membrane-daemon.exe", "plugin.json", "mcp.json", "skills", ".claude-plugin", ".codex-plugin", ".agents", ".antigravity-plugin", "LICENSE", "release.json", "THIRD_PARTY_NOTICES.md"]) {
    assert.ok(packager.includes(name), name);
  }
  assert.match(packager, /Get-AuthenticodeSignature/);
  assert.match(packager, /release_generation/);
  assert.match(packager, /assemblePortableCore/);
  assert.match(packager, /validatePortableCore/);
  assert.match(packager, /clientProjections: CLIENT_PROJECTION_KINDS/);
  assert.match(packager, /createPortableArchive/);
  assert.match(packager, /materializeCycloneDxSbom/);
  assert.match(packager, /materializeInTotoSlsaProvenance/);
  assert.match(packager, /membrane-\$\{pkg\.version\}-windows_x86_64|membrane-\$\{pkg\.version\}-windows-x86_64/);
});

test("shared bootstrap owns signed manifest, stable current, activation & exact health", () => {
  for (const term of ["collectReleaseAsset", "materializeDirectRelease", "renderPowerShellBootstrap", "validatePowerShellBootstrap", "planBootstrapPublication", "{current}", "schemaVersion", "dryRun", "membrane-hub", "releaseGeneration", "clients"]) {
    assert.ok(finalizer.includes(term), term);
  }
  assert.match(finalizer, /activate.*--install-root/s);
  assert.match(finalizer, /Orthic-Labs\/Membrane/);
  assert.doesNotMatch(finalizer, /New-Service|sc\.exe|Win32_Service/);
});

test("ported native client descriptors remain valid JSON", () => {
  for (const path of ["../../../.claude-plugin/plugin.json", "../../../.claude-plugin/marketplace.json", "../../../.codex-plugin/plugin.json", "../../../.antigravity-plugin/plugin.json", "../../../.antigravity-plugin/mcp_config.json"]) {
    assert.doesNotThrow(() => JSON.parse(readFileSync(new URL(path, import.meta.url), "utf8")));
  }
});

test("publication delegates GitHub payload & R2 bootstrap effects to RightRelease", () => {
  assert.equal(pkg.scripts["release:publish:portable:win"], "node scripts/publish-portable-release.mjs");
  for (const term of ["prepareGitHubDirectRelease", "publishGitHubRelease", "planBootstrapPublication", "createWranglerR2Client", "publishBootstrapPlan", "--dry-run"]) {
    assert.ok(publisher.includes(term), term);
  }
  assert.ok(publisher.indexOf("const github = publishGitHubRelease") < publisher.indexOf("const r2 = publishBootstrapPlan"));
  assert.doesNotMatch(publisher, /Invoke-WebRequest|wrangler@|r2 object|gh release/);
});
