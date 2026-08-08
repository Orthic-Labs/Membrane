import test from "node:test";
import assert from "node:assert/strict";
import { createHash } from "node:crypto";
import { mkdtempSync, mkdirSync, writeFileSync, readFileSync, rmSync } from "node:fs";
import { tmpdir } from "node:os";
import { join, resolve } from "node:path";
import { spawnSync } from "node:child_process";

import {
  HOMEBREW_CONTRACT_SCHEMA, CASK_PATH, FORMULA_PATH, CONTRACT_PATH,
  CASK_PLACEHOLDER, FORMULA_PLACEHOLDER, DEFAULT_PUBLIC_BASE, PUBLIC_BASE_ENV,
  readReleaseConfig, releaseIdFor, readSealedManifest,
  classifyMacArtifact, resolveCaskArtifacts, resolveFormulaArtifacts,
  renderCask, renderFormula, validateCaskSource, validateFormulaSource,
} from "../../../scripts/release/homebrew/generate.mjs";

const root = resolve(import.meta.dirname, "../../..");
const script = resolve(root, "scripts/release/homebrew/generate.mjs");
const sha256 = (bytes) => createHash("sha256").update(bytes).digest("hex");

// ---------------------------------------------------------------------------
// Fixture builders. No fixture is a static committed file: every sealed
// manifest and DMG byte used here is generated at test time so nothing can
// silently drift from the code that reads it.
// ---------------------------------------------------------------------------
function makeCommit(tag) { return `${tag}${"0".repeat(40 - tag.length)}`; }

function writeConfig(dir, { app, version, artifacts }) {
  writeFileSync(join(dir, "package.json"), JSON.stringify({ name: app, version }));
  const body = `import { readFileSync } from "node:fs";
const packageJson = JSON.parse(readFileSync(new URL("./package.json", import.meta.url), "utf8"));
export default {
  schema: 1,
  app: ${JSON.stringify(app)},
  version: packageJson.version,
  targets: { mac: { installer: { artifacts: ${JSON.stringify(artifacts)} } } },
};
`;
  const configPath = join(dir, "right-release.config.mjs");
  writeFileSync(configPath, body);
  return configPath;
}

function writeSealed({ sealedRoot, app, version, commit, files, routes }) {
  const id = releaseIdFor({ app, version, commit });
  const platformDir = join(sealedRoot, id, "mac");
  mkdirSync(platformDir, { recursive: true });
  const fileEntries = [];
  for (const file of files) {
    writeFileSync(join(platformDir, file.name), file.bytes);
    fileEntries.push({ role: "installer", name: file.name, sha256: sha256(file.bytes), sizeBytes: file.bytes.length });
  }
  const manifest = { schema: 1, releaseId: id, app, version, commit, platform: "mac", files: fileEntries, routes, checkpoints: ["preflight_complete", "build_complete", "signed", "hardened", "sealed"], sealedAt: new Date().toISOString() };
  writeFileSync(join(platformDir, "release-manifest.json"), JSON.stringify(manifest, null, 2));
  return { id, platformDir, manifest };
}

let workdir;
test.beforeEach(() => { workdir = mkdtempSync(join(tmpdir(), "mbr904-homebrew-")); });
test.afterEach(() => { rmSync(workdir, { recursive: true, force: true }); });

// ---------------------------------------------------------------------------
// The real, committed apps/membrane-hub/right-release.config.mjs — proves
// today's actual gap this task must respect: arm64 only, no Intel/x86_64
// mac target, and no headless CLI/daemon tarball target at all.
// ---------------------------------------------------------------------------
test("the real right-release.config.mjs declares arm64-only mac artifacts (the declared Intel gap MBR-904 must respect)", async () => {
  const config = await readReleaseConfig(resolve(root, "apps/membrane-hub/right-release.config.mjs"));
  assert.equal(config.app, "membrane-hub");
  const artifacts = config.targets.mac.installer.artifacts;
  assert.ok(artifacts.length >= 1);
  for (const artifact of artifacts) assert.equal(classifyMacArtifact(artifact.file), "mac-arm64");
  assert.ok(!artifacts.some((artifact) => classifyMacArtifact(artifact.file) === "mac-x64"), "no Intel/x86_64 mac target is declared today");
  assert.equal(config.targets.mac.headless, undefined, "no headless CLI/daemon target is declared today");
});

test("resolveFormulaArtifacts against the real config reports both architectures as an explicit, reasoned gap", async () => {
  const config = await readReleaseConfig(resolve(root, "apps/membrane-hub/right-release.config.mjs"));
  const resolved = resolveFormulaArtifacts({ config });
  assert.equal(resolved.ready, false);
  for (const arch of ["mac-arm64", "mac-x64"]) {
    assert.equal(resolved.arches[arch].blocked, true);
    assert.match(resolved.arches[arch].reason, /no headless CLI\/daemon release target/);
  }
});

// ---------------------------------------------------------------------------
// Single-arch fixture: mirrors today's real shape (arm64 DMG only).
// ---------------------------------------------------------------------------
test("a single-arch (arm64-only) sealed release resolves arm64 but leaves mac-x64 explicitly blocked, never silently assumed", () => {
  const commit = makeCommit("deadbeef");
  const app = "membrane-hub"; const version = "9.9.9";
  const dmgBytes = Buffer.from("FIXTURE-ARM64-DMG-CONTENT\n");
  const sealedRoot = join(workdir, "sealed");
  const { platformDir, manifest } = writeSealed({
    sealedRoot, app, version, commit,
    files: [{ name: `Fixture Hub_${version}_aarch64.dmg`, bytes: dmgBytes }],
    routes: { patch: [{ role: "installer", name: `Fixture Hub_${version}_aarch64.dmg`, bucket: "public", key: "membrane/installers/mac/current/Membrane_Hub.dmg" }], update: [] },
  });

  const config = { schema: 1, app, version, targets: { mac: { installer: { artifacts: [{ file: `src-tauri/target/release/bundle/dmg/Fixture Hub_${version}_aarch64.dmg`, key: "membrane/installers/mac/current/Membrane_Hub.dmg" }] } } } };
  const reread = readSealedManifest(platformDir);
  assert.deepEqual(reread, manifest);

  const resolved = resolveCaskArtifacts({ config, manifest: reread, publicBase: DEFAULT_PUBLIC_BASE });
  assert.equal(resolved.ready, false, "single-arch data must never be reported ready");
  assert.equal(resolved.arches["mac-arm64"].blocked, undefined);
  assert.equal(resolved.arches["mac-arm64"].sha256, sha256(dmgBytes));
  assert.equal(resolved.arches["mac-arm64"].url, `${DEFAULT_PUBLIC_BASE}/membrane/installers/mac/current/Membrane_Hub.dmg`);
  assert.equal(resolved.arches["mac-x64"].blocked, true);
  assert.match(resolved.arches["mac-x64"].reason, /declares no x86_64\/Intel mac installer target/);

  // renderCask/renderFormula must fall back to the exact safe placeholder.
  assert.equal(renderCask(resolved), CASK_PLACEHOLDER);
  const formulaResolved = resolveFormulaArtifacts({ config });
  assert.equal(renderFormula(formulaResolved), FORMULA_PLACEHOLDER);
});

test("a sealed file whose bytes do not match its recorded sha256 fails closed before any Cask is generated", () => {
  const commit = makeCommit("badbadba");
  const app = "membrane-hub"; const version = "9.9.9";
  const sealedRoot = join(workdir, "sealed");
  const { platformDir } = writeSealed({
    sealedRoot, app, version, commit,
    files: [{ name: "Fixture Hub_9.9.9_aarch64.dmg", bytes: Buffer.from("original") }],
    routes: { patch: [{ role: "installer", name: "Fixture Hub_9.9.9_aarch64.dmg", bucket: "public", key: "membrane/installers/mac/current/Membrane_Hub.dmg" }], update: [] },
  });
  writeFileSync(join(platformDir, "Fixture Hub_9.9.9_aarch64.dmg"), Buffer.from("tampered-after-seal"));
  assert.throws(() => readSealedManifest(platformDir), /sealed file hash mismatch/);
});

test("an unsealed or schema-invalid manifest fails closed", () => {
  const dir = join(workdir, "unsealed"); mkdirSync(dir, { recursive: true });
  writeFileSync(join(dir, "release-manifest.json"), JSON.stringify({ schema: 1, files: [], checkpoints: ["build_complete"] }));
  assert.throws(() => readSealedManifest(dir), /release is not sealed/);
  assert.throws(() => readSealedManifest(join(workdir, "missing")), /sealed manifest missing/);
});

// ---------------------------------------------------------------------------
// Dual-arch fixture: proves the generator's real-artifact rendering path is
// correct and forward-ready for when RightKit ships an Intel mac target —
// entirely synthetic, never presented as today's real release.
// ---------------------------------------------------------------------------
test("a dual-arch (arm64 + x64) sealed release is ready and renders a Cask/Formula that handle both architectures explicitly", () => {
  const commit = makeCommit("cafef00d");
  const app = "membrane-hub-fixture"; const version = "0.0.1";
  const armBytes = Buffer.from("FIXTURE-DUAL-ARM64-DMG-CONTENT\n");
  const x64Bytes = Buffer.from("FIXTURE-DUAL-X64-DMG-CONTENT\n");
  const sealedRoot = join(workdir, "sealed");
  const { platformDir, manifest } = writeSealed({
    sealedRoot, app, version, commit,
    files: [
      { name: `Fixture Hub_${version}_aarch64.dmg`, bytes: armBytes },
      { name: `Fixture Hub_${version}_x86_64.dmg`, bytes: x64Bytes },
    ],
    routes: {
      patch: [
        { role: "installer", name: `Fixture Hub_${version}_aarch64.dmg`, bucket: "public", key: "membrane-fixture/installers/mac/current/Fixture_Hub_arm64.dmg" },
        { role: "installer", name: `Fixture Hub_${version}_x86_64.dmg`, bucket: "public", key: "membrane-fixture/installers/mac/current/Fixture_Hub_x64.dmg" },
      ],
      update: [],
    },
  });
  const config = {
    schema: 1, app, version,
    targets: { mac: { installer: { artifacts: [
      { file: `.../Fixture Hub_${version}_aarch64.dmg`, key: "membrane-fixture/installers/mac/current/Fixture_Hub_arm64.dmg" },
      { file: `.../Fixture Hub_${version}_x86_64.dmg`, key: "membrane-fixture/installers/mac/current/Fixture_Hub_x64.dmg" },
    ] } } },
  };
  const reread = readSealedManifest(platformDir);
  const resolved = resolveCaskArtifacts({ config, manifest: reread, publicBase: "https://example-fixture.r2.dev" });
  assert.equal(resolved.ready, true);
  assert.equal(resolved.arches["mac-arm64"].sha256, sha256(armBytes));
  assert.equal(resolved.arches["mac-x64"].sha256, sha256(x64Bytes));
  assert.equal(resolved.arches["mac-arm64"].url, "https://example-fixture.r2.dev/membrane-fixture/installers/mac/current/Fixture_Hub_arm64.dmg");
  assert.equal(resolved.arches["mac-x64"].url, "https://example-fixture.r2.dev/membrane-fixture/installers/mac/current/Fixture_Hub_x64.dmg");

  const caskText = renderCask(resolved);
  assert.notEqual(caskText, CASK_PLACEHOLDER);
  assert.doesNotThrow(() => validateCaskSource(caskText, resolved));
  assert.match(caskText, /on_arm do/);
  assert.match(caskText, /on_intel do/);
  assert.ok(caskText.includes(resolved.arches["mac-arm64"].sha256));
  assert.ok(caskText.includes(resolved.arches["mac-x64"].sha256));

  const formulaResolved = { version, arches: resolved.arches, ready: true };
  const formulaText = renderFormula(formulaResolved);
  assert.notEqual(formulaText, FORMULA_PLACEHOLDER);
  assert.doesNotThrow(() => validateFormulaSource(formulaText, formulaResolved));
  assert.match(formulaText, /Hardware::CPU\.arm\?/);
  assert.match(formulaText, /service do/);
});

test("renderCask/renderFormula refuse to render real output for malformed resolved data even when ready:true is forced", () => {
  const badSha = { ready: true, version: "1.2.3", arches: { "mac-arm64": { url: "https://x.example/a.dmg", sha256: "not-a-hash" }, "mac-x64": { url: "https://x.example/b.dmg", sha256: "b".repeat(64) } } };
  assert.throws(() => renderCask(badSha), /sha256/);
  const badUrl = { ready: true, version: "1.2.3", arches: { "mac-arm64": { url: "http://insecure.example/a.dmg", sha256: "a".repeat(64) }, "mac-x64": { url: "https://x.example/b.dmg", sha256: "b".repeat(64) } } };
  assert.throws(() => renderCask(badUrl), /https/);
});

// ---------------------------------------------------------------------------
// The committed Cask/Formula/contract, and the validators over them.
// ---------------------------------------------------------------------------
test("the committed Cask and Formula are exactly the generator's not-ready placeholder", () => {
  assert.equal(readFileSync(resolve(root, CASK_PATH), "utf8"), CASK_PLACEHOLDER);
  assert.equal(readFileSync(resolve(root, FORMULA_PATH), "utf8"), FORMULA_PLACEHOLDER);
});

test("validateCaskSource/validateFormulaSource accept the committed not-ready placeholders and reject a source that leaks real data while not ready", () => {
  const cask = readFileSync(resolve(root, CASK_PATH), "utf8");
  const formula = readFileSync(resolve(root, FORMULA_PATH), "utf8");
  assert.doesNotThrow(() => validateCaskSource(cask, { ready: false }));
  assert.doesNotThrow(() => validateFormulaSource(formula, { ready: false }));
  assert.throws(() => validateCaskSource(`${cask}\n  url "https://example.com/a.dmg"\n`, { ready: false }), /must not embed a url/);
  assert.throws(() => validateFormulaSource(`${formula}\n  sha256 "${"a".repeat(64)}"\n`, { ready: false }), /must not embed a sha256/);
});

test("the committed packaging/homebrew/release.v1.json declares both mac architectures with no real evidence yet", () => {
  const contract = JSON.parse(readFileSync(resolve(root, CONTRACT_PATH), "utf8"));
  assert.equal(contract.schema, "orthic.membrane.homebrew-release.v1");
  assert.equal(HOMEBREW_CONTRACT_SCHEMA, contract.schema);
  assert.deepEqual(Object.keys(contract.platforms).sort(), ["mac-arm64", "mac-x64"]);
  assert.equal(contract.state, "unavailable");
});

// ---------------------------------------------------------------------------
// CLI — argument/contract validation only. Never touches brew, git, or any
// signing tool; only reads fixture files this test itself created.
// ---------------------------------------------------------------------------
test("CLI plan rejects a malformed --commit before reading any config or manifest", () => {
  const result = spawnSync(process.execPath, [script, "plan", "--commit", "not-a-sha"], { encoding: "utf8" });
  assert.equal(result.status, 2);
  assert.match(result.stderr, /--commit must be a 40-hex sha/);
});

test("CLI plan fails closed when the sealed manifest for the given commit does not exist", () => {
  const commit = makeCommit("11112222");
  const configPath = writeConfig(workdir, { app: "membrane-hub", version: "9.9.9", artifacts: [{ file: "x", key: "membrane/installers/mac/current/Membrane_Hub.dmg" }] });
  const result = spawnSync(process.execPath, [script, "plan", "--commit", commit, "--config", configPath, "--sealed-root", join(workdir, "sealed")], { encoding: "utf8" });
  assert.equal(result.status, 2);
  assert.match(result.stderr, /sealed manifest invalid/);
});

test("CLI plan on real fixture inputs reports arm64 resolved and mac-x64 explicitly blocked, and never writes any file without --out", () => {
  const commit = makeCommit("33334444");
  const app = "membrane-hub"; const version = "9.9.9";
  const configPath = writeConfig(workdir, { app, version, artifacts: [{ file: `.../Fixture Hub_${version}_aarch64.dmg`, key: "membrane/installers/mac/current/Membrane_Hub.dmg" }] });
  const dmgBytes = Buffer.from("FIXTURE-ARM64-DMG-CONTENT\n");
  writeSealed({
    sealedRoot: join(workdir, "sealed"), app, version, commit,
    files: [{ name: `Fixture Hub_${version}_aarch64.dmg`, bytes: dmgBytes }],
    routes: { patch: [{ role: "installer", name: `Fixture Hub_${version}_aarch64.dmg`, bucket: "public", key: "membrane/installers/mac/current/Membrane_Hub.dmg" }], update: [] },
  });
  const result = spawnSync(process.execPath, [script, "plan", "--commit", commit, "--config", configPath, "--sealed-root", join(workdir, "sealed"), "--public-base", "https://example-fixture.r2.dev"], { encoding: "utf8" });
  assert.equal(result.status, 0, result.stderr);
  const plan = JSON.parse(result.stdout);
  assert.equal(plan.cask.ready, false);
  assert.equal(plan.cask.arches["mac-arm64"].sha256, sha256(dmgBytes));
  assert.equal(plan.cask.arches["mac-x64"].blocked, true);
  assert.equal(plan.formula.ready, false);
});

test("CLI render reproduces the exact committed placeholder for the current not-ready contract shape", () => {
  const contract = { cask: { ready: false }, formula: { ready: false } };
  const contractPath = join(workdir, "contract.json");
  writeFileSync(contractPath, JSON.stringify(contract));
  const result = spawnSync(process.execPath, [script, "render", "--contract", contractPath], { encoding: "utf8" });
  assert.equal(result.status, 0, result.stderr);
  assert.ok(result.stdout.includes(CASK_PLACEHOLDER));
  assert.ok(result.stdout.includes(FORMULA_PLACEHOLDER));
});

test("CLI validate passes against the real committed Cask/Formula with no contract given", () => {
  const result = spawnSync(process.execPath, [script, "validate"], { encoding: "utf8" });
  assert.equal(result.status, 0, result.stderr);
  assert.match(result.stdout, /validated/);
});
