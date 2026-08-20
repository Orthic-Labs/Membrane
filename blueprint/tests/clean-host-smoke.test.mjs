import assert from "node:assert/strict";
import { spawnSync } from "node:child_process";
import { createHash, generateKeyPairSync } from "node:crypto";
import { existsSync, mkdirSync, mkdtempSync, readFileSync, readdirSync, rmSync, writeFileSync } from "node:fs";
import { tmpdir } from "node:os";
import { join } from "node:path";
import { fileURLToPath } from "node:url";
import test from "node:test";

import { buildCandidate } from "../scripts/release/build-candidate.mjs";
import { verifyCandidate } from "../scripts/release/check-release.mjs";
import { deriveUpdateKeyId } from "../scripts/release/generate-update-keys.mjs";

const ROOT = fileURLToPath(new URL("..", import.meta.url));
const SMOKE = fileURLToPath(new URL("../scripts/release/clean-host-smoke.mjs", import.meta.url));

test("clean-host harness is present as a release executable", () => {
  assert.ok(existsSync(SMOKE), "missing scripts/release/clean-host-smoke.mjs");
});

test("RightGit owns CI publication while signing remains local", () => {
  const workflows = join(ROOT, ".github", "workflows");
  const rightGit = JSON.parse(readFileSync(join(ROOT, ".rightgit.json"), "utf8"));
  assert.deepEqual(rightGit.lanes, ["ci", "publish-npm"]);
  assert.equal(rightGit.publish.npm.oidc, true);
  assert.equal(rightGit.publish.npm.provenance, true);
  assert.equal(existsSync(join(workflows, "immutable-release.yml")), false);
  assert.equal(existsSync(join(workflows, "release.yml")), false);
  const publish = readFileSync(join(workflows, "publish-npm.yml"), "utf8").replaceAll("\r\n", "\n");
  assert.match(publish, /^# Managed by right-git/m);
  assert.match(publish, /release:\n    types: \[published\]/);
  assert.match(publish, /id-token: write/);
  assert.match(publish, /build-candidate\.mjs/);
  assert.match(publish, /check-release\.mjs/);
  assert.match(publish, /npm publish .*--provenance --access public/);
  const allWorkflows = readdirSync(workflows).filter((file) => file.endsWith(".yml")).map((file) => readFileSync(join(workflows, file), "utf8")).join("\n");
  assert.doesNotMatch(allWorkflows, /APPLE_|AZURE_|UPDATE_SIGNING_KEY|NPM_TOKEN|artifact-signing-action|notarytool/i);
});

test("query evidence rejects an empty result set", async () => {
  const smoke = await import("../scripts/release/clean-host-smoke.mjs");
  assert.equal(typeof smoke.validateQueryEvidence, "function");
  assert.equal(typeof smoke.resolveShippedSigningKey, "function");
  if (typeof smoke.validateQueryEvidence === "function") {
    assert.throws(() => smoke.validateQueryEvidence({ results: [] }), /releaseProof/);
    assert.throws(() => smoke.validateQueryEvidence({ results: [{ id: "releaseProof", payload: {} }] }), /releaseProof/);
  }
});

test("shipped signing key must be present in the shipped trust root or fail clearly", async () => {
  const { resolveShippedSigningKey } = await import("../scripts/release/clean-host-smoke.mjs");
  const { publicKey, privateKey } = generateKeyPairSync("ed25519");
  const keyId = deriveUpdateKeyId(publicKey);
  const publicPem = publicKey.export({ type: "spki", format: "pem" });
  const privatePem = privateKey.export({ type: "pkcs8", format: "pem" });
  assert.equal(resolveShippedSigningKey({ [keyId]: publicPem }, privatePem), keyId);
  assert.throws(() => resolveShippedSigningKey({ [keyId]: publicPem }, null), /UPDATE_SIGNING_KEY_PEM/);
  assert.throws(() => resolveShippedSigningKey({}, privatePem), /not present in the shipped trust root/);
  assert.throws(() => resolveShippedSigningKey({ other: publicPem }, privatePem), /not present in the shipped trust root/);
});

test("clean-host rehearsal runs installed package stages", async () => {
  const out = mkdtempSync(join(tmpdir(), "blueprint-clean-host-candidate-"));
  try {
    buildCandidate({ out, allowDirty: true });
    const { runCleanHostSmoke } = await import("../scripts/release/clean-host-smoke.mjs");
    const report = await runCleanHostSmoke({ candidate: out });
    assert.equal(report.ok, true);
    assert.deepEqual(Object.keys(report.stages).sort(), ["init", "mcp", "query", "rollback", "uninstall", "updateApply", "updateCheck", "updateTrustMissing", "verify"].sort());
    assert.ok(Object.values(report.stages).every(Boolean));
    assert.equal(report.trustRoot.source, "ephemeral", "shipped trust root is empty; the ephemeral key path must run");
    assert.equal(report.trustRoot.keyCount, 0);
    assert.ok(report.queryEvidence.matchCount > 0);
    assert.ok(report.queryEvidence.refs.length > 0);
    assert.deepEqual(report.storeEvidence, { before: "before", after: "after", restored: "before" });
  } finally {
    rmSync(out, { recursive: true, force: true });
  }
});

function readJson(path) { return JSON.parse(readFileSync(path, "utf8")); }

function sha256File(path) { return createHash("sha256").update(readFileSync(path)).digest("hex"); }

function repackWithShippedRoot(tarballPath, keys) {
  const dir = join(tmpdir(), `blueprint-clean-host-repack-${process.pid}-${Math.random().toString(36).slice(2)}`);
  mkdirSync(dir, { recursive: true });
  const unpacked = join(dir, "unpacked");
  mkdirSync(unpacked, { recursive: true });
  const extract = spawnSync("tar", ["-xzf", tarballPath, "-C", unpacked], { encoding: "utf8" });
  if (extract.status !== 0) throw new Error(`tar extract failed: ${extract.stderr || extract.stdout}`);
  writeFileSync(join(unpacked, "package", "src", "lib", "update", "trusted-update-keys.json"), `${JSON.stringify({ schemaVersion: 1, keys })}\n`);
  const repack = spawnSync("tar", ["-czf", join(dir, "repacked.tgz"), "-C", unpacked, "package"], { encoding: "utf8" });
  if (repack.status !== 0) throw new Error(`tar repack failed: ${repack.stderr || repack.stdout}`);
  const packed = join(dir, "repacked.tgz");
  writeFileSync(tarballPath, readFileSync(packed));
  rmSync(dir, { recursive: true, force: true });
  return { path: tarballPath, checksum: sha256File(tarballPath) };
}

test("clean-host rehearsal applies and rolls back against a shipped trust root without overwriting it", async () => {
  const out = mkdtempSync(join(tmpdir(), "blueprint-clean-host-candidate-"));
  try {
    buildCandidate({ out, allowDirty: true });
    const compatibilityPath = join(out, "compatibility.json");
    const checksumsPath = join(out, "checksums.txt");
    const compatibility = readJson(compatibilityPath);
    const tarballEntry = compatibility.artifacts.find((artifact) => artifact.name.endsWith(".tgz"));
    assert.ok(tarballEntry, "candidate must contain a tarball");
    // Tests use only generated test keys: the repacked tarball ships the
    // matching public key, and the private PEM is passed temporarily through
    // UPDATE_SIGNING_KEY_PEM (restored in finally).
    const { publicKey, privateKey } = generateKeyPairSync("ed25519");
    const keyId = deriveUpdateKeyId(publicKey);
    const publicPem = publicKey.export({ type: "spki", format: "pem" });
    const privatePem = privateKey.export({ type: "pkcs8", format: "pem" });
    const repacked = repackWithShippedRoot(join(out, tarballEntry.name), [{ keyId, algorithm: "Ed25519", publicKey: publicPem }]);
    compatibility.artifacts = compatibility.artifacts.map((artifact) => artifact.name === tarballEntry.name ? { ...artifact, sha256: repacked.checksum } : artifact);
    writeFileSync(compatibilityPath, `${JSON.stringify(compatibility, null, 2)}\n`);
    writeFileSync(checksumsPath, `${compatibility.artifacts.map((artifact) => `${artifact.sha256}  ${artifact.name}`).join("\n")}\n`);
    const updateManifestPath = join(out, "update-manifest.json"), updateManifest = readJson(updateManifestPath);
    updateManifest.artifacts = updateManifest.artifacts.map((artifact) => artifact.name === tarballEntry.name ? { ...artifact, sha256: repacked.checksum } : artifact);
    writeFileSync(updateManifestPath, `${JSON.stringify(updateManifest, null, 2)}\n`);
    assert.equal(verifyCandidate(out).ok, true, JSON.stringify(verifyCandidate(out).problems));
    const { runCleanHostSmoke } = await import("../scripts/release/clean-host-smoke.mjs");
    const previousPem = process.env.UPDATE_SIGNING_KEY_PEM;
    process.env.UPDATE_SIGNING_KEY_PEM = privatePem;
    try {
      const report = await runCleanHostSmoke({ candidate: out });
      assert.equal(report.ok, true);
      assert.deepEqual(Object.keys(report.stages).sort(), ["init", "mcp", "query", "rollback", "uninstall", "updateApply", "updateCheck", "updateTrustMissing", "verify"].sort());
      assert.ok(Object.values(report.stages).every(Boolean));
      assert.equal(report.trustRoot.source, "shipped", "a non-empty shipped trust root must be used");
      assert.equal(report.trustRoot.keyCount, 1);
      assert.equal(report.trustRoot.keyId, keyId, "the manifest must be signed with the key the shipped root holds");
      assert.equal(report.trustRoot.rootUntouched, true, "the shipped trust root must not be overwritten");
      assert.deepEqual(report.storeEvidence, { before: "before", after: "after", restored: "before" });
    } finally {
      if (previousPem === undefined) delete process.env.UPDATE_SIGNING_KEY_PEM;
      else process.env.UPDATE_SIGNING_KEY_PEM = previousPem;
    }
  } finally {
    rmSync(out, { recursive: true, force: true });
  }
});
