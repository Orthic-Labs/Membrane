// D13: release candidate contract — unsigned artifacts only, checksummed,
// inventoried, and SBOM-backed; no publishing.

import assert from "node:assert/strict";
import { execFileSync, spawnSync } from "node:child_process";
import { existsSync, mkdtempSync, mkdirSync, readFileSync, rmSync, symlinkSync, writeFileSync } from "node:fs";
import { createHash, generateKeyPairSync } from "node:crypto";
import { tmpdir } from "node:os";
import { join } from "node:path";
import test from "node:test";

import { SCHEMA_VERSION } from "../src/graph/store-sqlite.mjs";
import { buildCandidate, isDirty } from "../scripts/release/build-candidate.mjs";
import { verifyCandidate } from "../scripts/release/check-release.mjs";
import { verifyNativeReceipt } from "../scripts/release/verify-native-receipt.mjs";
import { loadUpdateManifest, verifyArtifactChecksum, verifySignedManifest } from "../src/lib/update/manifest.mjs";
import { signUpdateManifest } from "../scripts/release/sign-update-manifest.mjs";

const compatibilityFiles = ["compatibility.template.json"];

const STAGES = ["install", "init", "query", "mcp", "update", "rollback", "uninstall"];

function validReceipt(overrides = {}) {
  const stages = {};
  for (const stage of STAGES) stages[stage] = true;
  return {
    schemaVersion: 1,
    version: "0.2.0",
    platform: "darwin-arm64",
    artifactSha256: "a".repeat(64),
    stages,
    runnerIdentity: "github-hosted run 1",
    signedAt: "2026-08-08T00:00:00.000Z",
    ...overrides,
  };
}

function writeCatalog(dir) {
  const catalog = {
    schemaVersion: 1,
    product: "Blueprint",
    version: "0.2.0",
    artifacts: [{ name: "blueprint-darwin-arm64.tar.gz", platform: "darwin", arch: "arm64", sha256: "a".repeat(64) }],
  };
  writeFileSync(join(dir, "catalog.json"), `${JSON.stringify(catalog, null, 2)}\n`);
  return join(dir, "catalog.json");
}

test("native receipt verifier passes a schema-valid receipt with catalog hash and all stages true", () => {
  const dir = mkdtempSync(join(tmpdir(), "cx-receipt-valid-"));
  try {
    const receiptPath = join(dir, "receipt.json");
    writeFileSync(receiptPath, `${JSON.stringify(validReceipt(), null, 2)}\n`);
    const result = verifyNativeReceipt({ receiptPath, catalogPath: writeCatalog(dir), version: "0.2.0" });
    assert.equal(result.ok, true, result.problems?.join("; "));
  } finally { rmSync(dir, { recursive: true, force: true }); }
});

test("native receipt verifier fails on a missing receipt", () => {
  const dir = mkdtempSync(join(tmpdir(), "cx-receipt-missing-"));
  try {
    const result = verifyNativeReceipt({ receiptPath: join(dir, "absent.json"), catalogPath: writeCatalog(dir), version: "0.2.0" });
    assert.equal(result.ok, false);
    assert.match(result.problems.join("; "), /absent|unreadable/i);
  } finally { rmSync(dir, { recursive: true, force: true }); }
});

test("native receipt verifier fails on a wrong version", () => {
  const dir = mkdtempSync(join(tmpdir(), "cx-receipt-version-"));
  try {
    const receiptPath = join(dir, "receipt.json");
    writeFileSync(receiptPath, `${JSON.stringify(validReceipt({ version: "9.9.9" }), null, 2)}\n`);
    const result = verifyNativeReceipt({ receiptPath, catalogPath: writeCatalog(dir), version: "0.2.0" });
    assert.equal(result.ok, false);
    assert.match(result.problems.join("; "), /does not match/);
  } finally { rmSync(dir, { recursive: true, force: true }); }
});

test("native receipt verifier fails on a hash absent from the catalog", () => {
  const dir = mkdtempSync(join(tmpdir(), "cx-receipt-hash-"));
  try {
    const receiptPath = join(dir, "receipt.json");
    writeFileSync(receiptPath, `${JSON.stringify(validReceipt({ artifactSha256: "b".repeat(64) }), null, 2)}\n`);
    const result = verifyNativeReceipt({ receiptPath, catalogPath: writeCatalog(dir), version: "0.2.0" });
    assert.equal(result.ok, false);
    assert.match(result.problems.join("; "), /not present/);
  } finally { rmSync(dir, { recursive: true, force: true }); }
});

test("native receipt verifier fails when a stage is false", () => {
  const dir = mkdtempSync(join(tmpdir(), "cx-receipt-stage-"));
  try {
    const stages = {};
    for (const stage of STAGES) stages[stage] = true;
    stages.uninstall = false;
    const receiptPath = join(dir, "receipt.json");
    writeFileSync(receiptPath, `${JSON.stringify(validReceipt({ stages }), null, 2)}\n`);
    const result = verifyNativeReceipt({ receiptPath, catalogPath: writeCatalog(dir), version: "0.2.0" });
    assert.equal(result.ok, false);
    assert.match(result.problems.join("; "), /uninstall/);
  } finally { rmSync(dir, { recursive: true, force: true }); }
});

test("native receipt verifier CLI exits nonzero for a missing receipt and zero for a valid one", () => {
  const dir = mkdtempSync(join(tmpdir(), "cx-receipt-cli-"));
  try {
    const script = join(import.meta.dirname, "..", "scripts", "release", "verify-native-receipt.mjs");
    const catalogPath = writeCatalog(dir);
    const missing = spawnSync(process.execPath, [script, "--receipt", join(dir, "absent.json"), "--catalog", catalogPath, "--version", "0.2.0"], { encoding: "utf8" });
    assert.notEqual(missing.status, 0);
    assert.match(missing.stderr, /absent|unreadable/i);
    const receiptPath = join(dir, "receipt.json");
    writeFileSync(receiptPath, `${JSON.stringify(validReceipt(), null, 2)}\n`);
    const valid = spawnSync(process.execPath, [script, "--receipt", receiptPath, "--catalog", catalogPath, "--version", "0.2.0"], { encoding: "utf8" });
    assert.equal(valid.status, 0, valid.stderr);
    assert.match(valid.stdout, /verify-native-receipt OK/);
  } finally { rmSync(dir, { recursive: true, force: true }); }
});

test("release store contracts match current runtime schema", () => {
  const out = mkdtempSync(join(tmpdir(), "blueprint-rc-schema-"));
  try {
    const candidate = buildCandidate({ out, allowDirty: true });
    assert.equal(candidate.compatibility.contracts.store, SCHEMA_VERSION);
    for (const file of compatibilityFiles) {
      const compatibility = JSON.parse(readFileSync(new URL(`../release/${file}`, import.meta.url), "utf8"));
      assert.equal(compatibility.contracts.store, SCHEMA_VERSION, file);
      assert.equal(compatibility.storeMigration.currentSchemaVersion, SCHEMA_VERSION, file);
    }
  } finally {
    rmSync(out, { recursive: true, force: true });
  }
});

test("candidate build emits compatibility, checksums, SBOM, catalog, and update manifest", () => {
  const out = mkdtempSync(join(tmpdir(), "blueprint-rc-"));
  try {
    const result = buildCandidate({ out, allowDirty: true });
    assert.ok(result.compatibility);
    assert.equal(result.compatibility.schemaVersion, 1);
    assert.equal(result.compatibility.product, "Blueprint");
    assert.ok(result.compatibility.commit.length === 40);
    for (const file of ["compatibility.json", "checksums.txt", "SBOM.spdx.json", "artifact-catalog.json", "update-manifest.json", "THIRD_PARTY_NOTICES"]) {
      assert.ok(existsSync(join(out, file)), `missing ${file}`);
    }
  } finally {
    rmSync(out, { recursive: true, force: true });
  }
});

test("candidate update manifest is a signable UpdateManifestV1 bound to the artifacts", () => {
  const out = mkdtempSync(join(tmpdir(), "blueprint-rc-manifest-"));
  try {
    buildCandidate({ out, allowDirty: true });
    const manifest = loadUpdateManifest(join(out, "update-manifest.json"));
    assert.equal(manifest.schemaVersion, 1);
    assert.equal(manifest.channel, "stable");
    assert.equal(manifest.version, "0.2.0");
    assert.ok(manifest.publishedAt);
    assert.equal(manifest.signatureAlgorithm, "Ed25519");
    const compat = JSON.parse(readFileSync(join(out, "compatibility.json"), "utf8"));
    assert.equal(manifest.artifacts.length, compat.artifacts.length);
    for (let index = 0; index < manifest.artifacts.length; index++) {
      assert.equal(manifest.artifacts[index].name, compat.artifacts[index].name);
      assert.equal(manifest.artifacts[index].sha256, compat.artifacts[index].sha256);
    }
    const { publicKey, privateKey } = generateKeyPairSync("ed25519");
    signUpdateManifest(manifest, { privateKeyPem: privateKey.export({ type: "pkcs8", format: "pem" }) });
    assert.match(manifest.keyId, /^[a-f0-9]{16}$/);
    assert.equal(verifySignedManifest(manifest, { trustedKeys: { [manifest.keyId]: publicKey.export({ type: "spki", format: "pem" }) } }).ok, true);
    assert.equal(verifyArtifactChecksum(manifest, compat.artifacts[0].name, compat.artifacts[0].sha256).ok, true);
    assert.equal(verifyArtifactChecksum(manifest, compat.artifacts[0].name, "f".repeat(64)).ok, false);
  } finally { rmSync(out, { recursive: true, force: true }); }
});

test("candidate verification fails on an absent or tampered update manifest", () => {
  const out = mkdtempSync(join(tmpdir(), "blueprint-rc-manifest-bad-"));
  try {
    buildCandidate({ out, allowDirty: true });
    const manifestPath = join(out, "update-manifest.json");
    const manifest = JSON.parse(readFileSync(manifestPath, "utf8"));
    manifest.artifacts[0].sha256 = "f".repeat(64);
    writeFileSync(manifestPath, `${JSON.stringify(manifest, null, 2)}\n`);
    const tampered = verifyCandidate(out);
    assert.equal(tampered.ok, false);
    assert.match(tampered.problems.join("; "), /update manifest/);
    rmSync(out, { recursive: true, force: true });
    mkdirSync(out);
    buildCandidate({ out, allowDirty: true });
    rmSync(manifestPath);
    assert.equal(verifyCandidate(out).ok, false);
  } finally { rmSync(out, { recursive: true, force: true }); }
});

test("candidate inventories one installable npm tarball", () => {
  const out = mkdtempSync(join(tmpdir(), "blueprint-rc-tarball-"));
  try {
    const result = buildCandidate({ out, allowDirty: true });
    const tarballs = result.compatibility.artifacts.filter((artifact) => artifact.name.endsWith(".tgz"));
    assert.equal(tarballs.length, 1);
    assert.equal(result.compatibility.packageName, "@membrane/blueprint");
    assert.equal(result.compatibility.platform, `${process.platform}-${process.arch}`);
    assert.ok(existsSync(join(out, tarballs[0].name)));
    assert.equal(verifyCandidate(out).ok, true);
  } finally {
    rmSync(out, { recursive: true, force: true });
  }
});

test("candidate verification rejects an unlisted extra file", () => {
  const out = mkdtempSync(join(tmpdir(), "blueprint-rc-extra-"));
  try {
    buildCandidate({ out, allowDirty: true });
    writeFileSync(join(out, "extra.bin"), "unexpected");
    assert.equal(verifyCandidate(out).ok, false);
  } finally { rmSync(out, { recursive: true, force: true }); }
});

test("candidate build rejects a nonempty output directory", () => {
  const out = mkdtempSync(join(tmpdir(), "blueprint-rc-nonempty-"));
  try {
    writeFileSync(join(out, "leftover.txt"), "leftover");
    assert.throws(() => buildCandidate({ out, allowDirty: true }), /empty output/);
  } finally { rmSync(out, { recursive: true, force: true }); }
});

test("candidate build rejects any repository output outside release candidates", () => {
  const out = join(import.meta.dirname, "candidate-output");
  try {
    rmSync(out, { recursive: true, force: true });
    assert.throws(() => buildCandidate({ out, allowDirty: true }), /release\/candidates/);
  } finally { rmSync(out, { recursive: true, force: true }); }
});

test("candidate verification rejects duplicate checksum names", () => {
  const out = mkdtempSync(join(tmpdir(), "blueprint-rc-duplicate-"));
  try {
    buildCandidate({ out, allowDirty: true });
    const checksums = join(out, "checksums.txt");
    writeFileSync(checksums, `${readFileSync(checksums, "utf8")}${readFileSync(checksums, "utf8").split("\n")[0]}\n`);
    assert.equal(verifyCandidate(out).ok, false);
  } finally { rmSync(out, { recursive: true, force: true }); }
});

test("candidate verification rejects a tarball symlink entry", (t) => {
  const out = mkdtempSync(join(tmpdir(), "blueprint-rc-tar-symlink-"));
  try {
    const candidate = buildCandidate({ out, allowDirty: true }), tarball = candidate.compatibility.artifacts.find((artifact) => artifact.name.endsWith(".tgz")).name;
    const stage = join(out, "tar-stage"); mkdirSync(join(stage, "package"), { recursive: true });
    writeFileSync(join(stage, "package", "package.json"), JSON.stringify({ name: candidate.compatibility.packageName, version: candidate.compatibility.version }));
    try { symlinkSync("elsewhere", join(stage, "package", "escape")); } catch { t.skip("symlink privilege unavailable"); return; }
    execFileSync("tar", ["-czf", tarball, "-C", stage, "package"], { cwd: out }); rmSync(stage, { recursive: true, force: true });
    const hash = createHash("sha256").update(readFileSync(join(out, tarball))).digest("hex"), compatibility = JSON.parse(readFileSync(join(out, "compatibility.json"), "utf8"));
    compatibility.artifacts.find((artifact) => artifact.name === tarball).sha256 = hash;
    writeFileSync(join(out, "compatibility.json"), JSON.stringify(compatibility));
    writeFileSync(join(out, "checksums.txt"), readFileSync(join(out, "checksums.txt"), "utf8").replace(/^([a-f0-9]{64})(  .*\.tgz)$/m, `${hash}$2`));
    assert.equal(verifyCandidate(out).ok, false);
  } finally { rmSync(out, { recursive: true, force: true }); }
});

test("candidate verification passes for a fresh build", () => {
  const out = mkdtempSync(join(tmpdir(), "blueprint-rc-verify-"));
  try {
    buildCandidate({ out, allowDirty: true });
    const result = verifyCandidate(out);
    assert.equal(result.ok, true, result.problems?.join("; "));
    assert.ok(result.compatibility.artifacts.length > 0);
  } finally {
    rmSync(out, { recursive: true, force: true });
  }
});

test("candidate verification fails on a tampered checksum", () => {
  const out = mkdtempSync(join(tmpdir(), "blueprint-rc-tamper-"));
  try {
    buildCandidate({ out, allowDirty: true });
    const checksumsPath = join(out, "checksums.txt");
    const text = readFileSync(checksumsPath, "utf8");
    const tampered = text.replace(/^[a-f0-9]{64}/, `${"a".repeat(64)}`);
    writeFileSync(checksumsPath, tampered);
    const result = verifyCandidate(out);
    assert.equal(result.ok, false);
  } finally {
    rmSync(out, { recursive: true, force: true });
  }
});

test("candidate build rejects a dirty tree", () => {
  // Fixture-scoped: a fresh temp git repo, dirtied only by this test, so the
  // guard no longer depends on the live worktree's state.
  const fixture = mkdtempSync(join(tmpdir(), "blueprint-rc-dirty-fixture-"));
  const repo = join(fixture, "repo");
  mkdirSync(repo);
  try {
    execFileSync("git", ["init", "-b", "main"], { cwd: repo, stdio: "ignore" });
    execFileSync("git", ["config", "user.email", "test@example.com"], { cwd: repo, stdio: "ignore" });
    execFileSync("git", ["config", "user.name", "blueprint tests"], { cwd: repo, stdio: "ignore" });
    writeFileSync(join(repo, "file.txt"), "clean\n");
    execFileSync("git", ["add", "file.txt"], { cwd: repo, stdio: "ignore" });
    execFileSync("git", ["commit", "-m", "fixture baseline"], { cwd: repo, stdio: "ignore" });
    assert.equal(isDirty(repo), false);
    writeFileSync(join(repo, "file.txt"), "dirty\n");
    assert.equal(isDirty(repo), true);
    assert.throws(() => buildCandidate({ out: join(fixture, "should-not-run"), gitRoot: repo }), /clean working tree/);
  } finally { rmSync(fixture, { recursive: true, force: true }); }
});

test("candidate CLI accepts omitted optional version", () => {
  const out = mkdtempSync(join(tmpdir(), "blueprint-rc-cli-"));
  try {
    const script = join(import.meta.dirname, "..", "scripts", "release", "build-candidate.mjs");
    const result = spawnSync(process.execPath, [script, "--allow-dirty", "--platform", "current", "--out", out], { encoding: "utf8" });
    assert.equal(result.status, 0, result.stderr);
    assert.equal(verifyCandidate(out).ok, true);
  } finally { rmSync(out, { recursive: true, force: true }); }
});

test("U60: hub installer checksum verification rejects mismatched artifact", async () => {
  const { verifyChecksum } = await import("../scripts/release/verify-hub-installer.mjs");
  const tmp = mkdtempSync(join(tmpdir(), "hub-check-"));
  try {
    const goodFile = join(tmp, "good.bin");
    writeFileSync(goodFile, "hub-installer-content-v1");
    const goodChecksum = createHash("sha256").update(readFileSync(goodFile)).digest("hex");
    assert.equal(verifyChecksum(goodFile, goodChecksum), true);
    // Mismatched checksum should throw
    assert.throws(() => verifyChecksum(goodFile, "0".repeat(64)), /checksum mismatch/);
    // Also test downloadHubInstaller with mocked fetcher
    const { downloadHubInstaller } = await import("../scripts/release/verify-hub-installer.mjs");
    const outDir = join(tmp, "out");
    // Mocked fetcher creates a file with known content and correct checksum should pass
    const mockContent = "mock-hub-installer";
    const mockChecksum = createHash("sha256").update(mockContent).digest("hex");
    const dest = downloadHubInstaller({
      hubVersion: "0.1.0-test",
      checksum: mockChecksum,
      outDir,
      fetcher: (p) => writeFileSync(p, mockContent),
    });
    assert.ok(existsSync(dest));
    // Mismatched should throw
    assert.throws(() => downloadHubInstaller({
      hubVersion: "0.1.0-test2",
      checksum: "f".repeat(64),
      outDir: join(tmp, "out2"),
      fetcher: (p) => writeFileSync(p, mockContent),
    }), /checksum mismatch/);
  } finally { rmSync(tmp, { recursive: true, force: true }); }
});
