// MBR-910: proves dist/packaging/oci/release.v1.json can only be promoted to
// "ready" from a real MBR-903 release-generation.json plus real,
// already-on-disk evidence files -- and refuses (fails closed, no write) the
// moment any one of those is missing, mismatched, or tampered with. Every
// fixture lives under a throwaway temp directory (never the real repo's
// dist/packaging/oci/release.v1.json or docs/evidence/releases/), and nothing here
// builds, signs, or runs a container.
import assert from "node:assert/strict";
import test from "node:test";
import { createHash } from "node:crypto";
import { mkdtempSync, mkdirSync, readFileSync, writeFileSync } from "node:fs";
import { tmpdir } from "node:os";
import { join, resolve } from "node:path";

import { buildOciRelease, writeOciRelease, readReleaseGeneration } from "../../scripts/release/oci/generate-oci-release.mjs";
import { releaseId } from "../../scripts/release/identity.mjs";

const COMMIT = "a".repeat(40);
const TREE = "b".repeat(40);
const RELEASE_GENERATION = "c".repeat(64);
const VERSION = "1.2.3";
const TAG = "v1.2.3";
const APP = "membrane-hub";
const BASE = `cgr.dev/chainguard/static@sha256:${"d".repeat(64)}`;
const IMAGE = `ghcr.io/orthic/membrane@sha256:${"e".repeat(64)}`;

function sha256(bytes) {
  return createHash("sha256").update(bytes).digest("hex");
}

function tempRepoRoot() {
  const root = mkdtempSync(join(tmpdir(), "mbr-910-oci-"));
  mkdirSync(resolve(root, "packaging", "oci"), { recursive: true });
  mkdirSync(resolve(root, "docs", "evidence", "releases", `${APP}-${VERSION}-${COMMIT.slice(0, 8)}`), { recursive: true });
  mkdirSync(resolve(root, "evidence", "productization", "MBR-910"), { recursive: true });
  writeFileSync(resolve(root, "packaging", "oci", "Containerfile"), `FROM ${BASE}\nUSER 65532:65532\n`);
  return root;
}

function writeReleaseGeneration(root, overrides = {}) {
  const doc = {
    schema: "orthic.membrane.release-generation.v1",
    product: "Membrane",
    vector_dispatch: "CRYPT_VECTOR_DISPATCH_V2",
    publish: false,
    release: {
      app: APP, version: VERSION, tag: TAG, commit: COMMIT, tree: TREE,
      release_generation: RELEASE_GENERATION,
      releaseId: releaseId({ app: APP, version: VERSION, commit: COMMIT }),
    },
    targets: [{ target: "mac-arm64", status: "pending" }],
    ...overrides,
  };
  const path = resolve(root, "docs", "evidence", "releases", `${APP}-${VERSION}-${COMMIT.slice(0, 8)}`, "release-generation.json");
  writeFileSync(path, JSON.stringify(doc, null, 2));
  return path;
}

function proofFile(root, name, content) {
  const path = resolve(root, "evidence", "productization", "MBR-910", name);
  writeFileSync(path, content);
  return path;
}

function fullOptions(root, releaseGenerationPath) {
  return {
    repoRoot: root,
    releaseGenerationPath,
    image: IMAGE,
    base: BASE,
    binaryPath: proofFile(root, "membrane-linux-x64", "fake-binary-bytes"),
    sbomPath: proofFile(root, "sbom.json", "fake-sbom"),
    ed25519Path: proofFile(root, "signature.ed25519", "fake-sig"),
    cosignPath: proofFile(root, "cosign.bundle", "fake-cosign"),
    rootlessHealthPath: proofFile(root, "rootless-health.json", "fake-health"),
    secretScanPath: proofFile(root, "secret-scan.json", "fake-scan"),
  };
}

test("readReleaseGeneration accepts a real MBR-903 record and rejects a tampered releaseId", () => {
  const root = tempRepoRoot();
  const path = writeReleaseGeneration(root);
  const doc = readReleaseGeneration(path);
  assert.equal(doc.release.commit, COMMIT);

  const badPath = writeReleaseGeneration(root, { release: { app: APP, version: VERSION, tag: TAG, commit: COMMIT, tree: TREE, release_generation: RELEASE_GENERATION, releaseId: "hand-typed-wrong-id" } });
  assert.throws(() => readReleaseGeneration(badPath), /FAIL CLOSED/);
});

test("buildOciRelease binds identity to the real release-generation record and validates real evidence", () => {
  const root = tempRepoRoot();
  const genPath = writeReleaseGeneration(root);
  const options = fullOptions(root, genPath);
  const candidate = buildOciRelease(options);

  assert.equal(candidate.state, "ready");
  assert.equal(candidate.identity.tag, TAG);
  assert.equal(candidate.identity.commit, COMMIT);
  assert.equal(candidate.identity.release_generation, RELEASE_GENERATION);
  assert.equal(candidate.identity.artifact_sha256, sha256(readFileSync(options.binaryPath)));
  assert.equal(candidate.evidence.sbom.sha256, sha256(readFileSync(options.sbomPath)));
  assert.equal(candidate.image, IMAGE);
  assert.equal(candidate.base, BASE);
});

test("buildOciRelease fails closed when release-generation.json does not exist yet", () => {
  const root = tempRepoRoot();
  const options = fullOptions(root, resolve(root, "docs", "evidence", "releases", "does-not-exist", "release-generation.json"));
  assert.throws(() => buildOciRelease(options), /FAIL CLOSED/);
});

test("buildOciRelease fails closed when --base drifts from the Containerfile's FROM line", () => {
  const root = tempRepoRoot();
  const genPath = writeReleaseGeneration(root);
  const options = { ...fullOptions(root, genPath), base: `cgr.dev/chainguard/static@sha256:${"9".repeat(64)}` };
  assert.throws(() => buildOciRelease(options), /does not match/);
});

test("buildOciRelease fails closed when a required evidence file is missing", () => {
  const root = tempRepoRoot();
  const genPath = writeReleaseGeneration(root);
  const options = fullOptions(root, genPath);
  options.sbomPath = resolve(root, "evidence", "productization", "MBR-910", "does-not-exist.json");
  assert.throws(() => buildOciRelease(options), /FAIL CLOSED/);
});

test("buildOciRelease fails closed on a non-digest-pinned --image", () => {
  const root = tempRepoRoot();
  const genPath = writeReleaseGeneration(root);
  const options = { ...fullOptions(root, genPath), image: "ghcr.io/orthic/membrane:latest" };
  assert.throws(() => buildOciRelease(options), /digest-pinned/);
});

test("writeOciRelease writes dist/packaging/oci/release.v1.json, is idempotent, and refuses to overwrite a different ready release", () => {
  const root = tempRepoRoot();
  const genPath = writeReleaseGeneration(root);
  const options = fullOptions(root, genPath);

  const first = writeOciRelease(options);
  assert.equal(first.path, resolve(root, "packaging", "oci", "release.v1.json"));
  const onDisk = JSON.parse(readFileSync(first.path, "utf8"));
  assert.equal(onDisk.state, "ready");

  // Identical inputs a second time: succeeds, still matches.
  const second = writeOciRelease(options);
  assert.deepEqual(second.candidate, first.candidate);

  // A genuinely different ready release (new binary content) for the SAME
  // already-recorded ready release must be refused, not silently overwritten.
  writeFileSync(options.binaryPath, "different-binary-bytes");
  assert.throws(() => writeOciRelease(options), /already records a different ready dist/release/);
});

test("leaves an existing unavailable release.v1.json alone when generation fails", () => {
  const root = tempRepoRoot();
  const releasePath = resolve(root, "packaging", "oci", "release.v1.json");
  const unavailable = { schema: "orthic.membrane.oci-release.v1", state: "unavailable", publish: false, image: "registry.invalid/orthic/membrane", base: BASE, identity: { tag: "UNAVAILABLE", commit: "0".repeat(40), release_generation: "0".repeat(64), artifact_sha256: "0".repeat(64) }, evidence: { sbom: null, ed25519: null, cosign: null, rootlessHealth: null, secretScan: null } };
  writeFileSync(releasePath, `${JSON.stringify(unavailable, null, 2)}\n`);

  const options = fullOptions(root, resolve(root, "docs", "evidence", "releases", "does-not-exist", "release-generation.json"));
  assert.throws(() => writeOciRelease(options), /FAIL CLOSED/);
  assert.deepEqual(JSON.parse(readFileSync(releasePath, "utf8")), unavailable);
});
