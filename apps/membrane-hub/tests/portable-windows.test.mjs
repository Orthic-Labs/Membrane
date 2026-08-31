import assert from "node:assert/strict";
import { readFileSync } from "node:fs";
import test from "node:test";

const pkg = JSON.parse(readFileSync(new URL("../package.json", import.meta.url), "utf8"));
const release = readFileSync(new URL("../scripts/release-build-portable-windows.mjs", import.meta.url), "utf8");
const candidateBuild = readFileSync(new URL("../scripts/release-build-candidate-windows.mjs", import.meta.url), "utf8");
const candidateCheck = readFileSync(new URL("../scripts/release-check-candidate-windows.mjs", import.meta.url), "utf8");
const packager = readFileSync(new URL("../scripts/package-portable-windows.mjs", import.meta.url), "utf8");
const finalizer = readFileSync(new URL("../scripts/finalize-portable-release.mjs", import.meta.url), "utf8");
const installerBundler = readFileSync(new URL("../scripts/bundle-portable-installer-windows.mjs", import.meta.url), "utf8");
const publisher = readFileSync(new URL("../scripts/publish-portable-release.mjs", import.meta.url), "utf8");
const remediation = readFileSync(new URL("../../../engine/crates/membrane-adapt/src/remediation.rs", import.meta.url), "utf8");

test("public CI builds unsigned candidate & protected host seals it without recompiling", () => {
  assert.equal(pkg.scripts["release:build:portable:win"], "node scripts/release-build-portable-windows.mjs");
  assert.equal(pkg.scripts["release:candidate:build:win"], "node scripts/release-build-candidate-windows.mjs");
  assert.equal(pkg.scripts["release:candidate:check:win"], "node scripts/release-check-candidate-windows.mjs");
  assert.match(candidateBuild, /RIGHT_GIT_ARTIFACT_ROOT/);
  assert.match(candidateBuild, /GITHUB_ACTIONS/);
  assert.match(candidateBuild, /Orthic-Labs\/Membrane/);
  assert.match(candidateBuild, /createPortableArchive/);
  assert.match(candidateBuild, /materializeCycloneDxSbom/);
  assert.match(candidateBuild, /materializeInTotoSlsaProvenance/);
  assert.match(candidateBuild, /"cargo", \["build"/);
  assert.doesNotMatch(candidateBuild, /sign-windows|right-release/);
  assert.doesNotMatch(candidateBuild, /tauri["'],\s*["']bundle|setup\.exe/i);
  assert.match(candidateCheck, /candidate archive digest mismatch/);
  assert.match(candidateCheck, /candidate file closure mismatch/);
  assert.match(candidateCheck, /candidate evidence digest mismatch/);
  assert.match(release, /MEMBRANE_CANDIDATE_ROOT/);
  assert.match(release, /release:candidate:check:win/);
  assert.match(release, /right-release", "sign-windows"/);
  assert.match(release, /materializeHardeningEvidence/);
  assert.match(release, /--allow-evidence/);
  assert.match(release, /fileURLToPath\(import\.meta\.resolve\("@rightkit\/release\/hardeningscan\.mjs"\)\)/);
  assert.match(release, /run\(process\.execPath, \[hardeningScan, "--allow-evidence"/);
  assert.doesNotMatch(release, /right-release\.cmd.*hardening/s);
  assert.match(release, /root: repoRoot/);
  const evidence = /sourceEvidence:\s*"engine\/crates\/membrane-adapt\/src\/remediation\.rs:(\d+)"/.exec(release);
  assert.ok(evidence, "hardening allowance must cite remediation source");
  assert.match(remediation.split(/\r?\n/)[Number(evidence[1]) - 1], /system_prompt/);
  assert.match(release, /package-portable-windows\.mjs/);
  assert.match(release, /finalize-portable-release\.mjs/);
  assert.match(release, /bundle-portable-installer-windows\.mjs/);
  assert.doesNotMatch(release, /["']cargo["']|release:prepare:sidecars|rightkit:package/i);
  assert.match(installerBundler, /verifyNsisEmbeddedBinary/);
  assert.match(installerBundler, /Object\.fromEntries\(releaseFiles\.map/);
  assert.match(installerBundler, /resources,/);
  assert.match(installerBundler, /nsis-embedded-receipt\.json/);
  assert.match(installerBundler, /membrane-nsis-direct-release-embedding-v1/);
  assert.match(installerBundler, /tauri\.release-bundle\.conf\.json/);
  assert.match(installerBundler, /"--config", bundleConfig/);
  assert.match(installerBundler, /rmSync\(bundleConfig, \{ force: true \}\)/);
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
  assert.match(packager, /join\(projectionRoot, "LICENSE"\)/);
  assert.match(packager, /join\(projectionRoot, "THIRD_PARTY_NOTICES\.md"\)/);
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
  assert.match(finalizer, /allowLocalReleaseRoot:\s*true/);
  assert.match(finalizer, /validatePowerShellBootstrap\(bootstrap, \{[\s\S]*allowLocalReleaseRoot:\s*true/);
  assert.ok(finalizer.includes("release-manifest-signing.json"));
  assert.doesNotMatch(finalizer, /New-Service|sc\.exe|Win32_Service/);
});

test("ported native client descriptors remain valid JSON", () => {
  for (const path of ["../../../.claude-plugin/plugin.json", "../../../.claude-plugin/marketplace.json", "../../../.codex-plugin/plugin.json", "../../../.antigravity-plugin/plugin.json", "../../../.antigravity-plugin/mcp_config.json"]) {
    assert.doesNotThrow(() => JSON.parse(readFileSync(new URL(path, import.meta.url), "utf8")));
  }
});

test("publication delegates GitHub payload & R2 bootstrap effects to RightRelease", () => {
  assert.equal(pkg.scripts["release:publish:portable:win"], "node scripts/publish-portable-release.mjs");
  assert.ok(publisher.includes('signaturePath: join(output, "release-manifest.cat")'));
  assert.ok(publisher.includes("release-manifest-signing.json"));
  assert.ok(publisher.includes("signing,"));
  for (const term of ["prepareGitHubDirectRelease", "publishGitHubRelease", "planBootstrapPublication", "createWranglerR2Client", "publishBootstrapPlan", "--dry-run"]) {
    assert.ok(publisher.includes(term), term);
  }
  assert.ok(publisher.indexOf("const github = publishGitHubRelease") < publisher.indexOf("const r2 = publishBootstrapPlan"));
  assert.doesNotMatch(publisher, /Invoke-WebRequest|wrangler@|r2 object|gh release/);
});
