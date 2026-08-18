import test from "node:test";
import assert from "node:assert/strict";
import { artifactDigest, bootstrap, selectPlatform, verifyArtifact, UnsupportedPlatformError, ArtifactVerificationError } from "../../dist/npm/index.mjs";

const artifact = Buffer.from("native-fixture");
const metadata = { sha256: artifactDigest(artifact), packageName: "@orthic/membrane-linux-x64", platform: "linux-x64", version: "0.1.0", signature: { algorithm: "ed25519", keyId: "fixture-key", value: "fixture-signature", digest: artifactDigest(artifact) } };

test("selectPlatform maps supported host tuples", () => {
  assert.deepEqual(selectPlatform({ platform: "darwin", arch: "arm64" }), { key: "darwin-arm64", packageName: "@orthic/membrane-darwin-arm64" });
});

test("unsupported hosts fail closed", () => {
  assert.throws(() => selectPlatform({ platform: "freebsd", arch: "x64" }), UnsupportedPlatformError);
});

test("digest and signed metadata are checked before dispatch", async () => {
  const calls = [];
  const result = await bootstrap({ artifact, metadata, platform: "linux", arch: "x64", verifySignature: async input => { calls.push(input); return true; }, load: async name => ({ dispatch: value => `${name}:${value}` }) });
  assert.equal(result.dispatch("ok"), "@orthic/membrane-linux-x64:ok");
  assert.equal(calls.length, 1);
});

test("digest mismatch prevents package loading", async () => {
  let loaded = false;
  await assert.rejects(() => bootstrap({ artifact, metadata: { ...metadata, sha256: "sha256:" + "0".repeat(64) }, load: async () => { loaded = true; } }), ArtifactVerificationError);
  assert.equal(loaded, false);
});

test("signature verification and package binding are mandatory", async () => {
  await assert.rejects(() => bootstrap({ artifact, metadata, platform: "linux", arch: "x64", load: async () => ({ dispatch() {} }) }), ArtifactVerificationError);
  await assert.rejects(() => bootstrap({ artifact, metadata: { ...metadata, packageName: "@orthic/other" }, platform: "linux", arch: "x64", verifySignature: async () => true }), ArtifactVerificationError);
  await assert.rejects(() => verifyArtifact({ artifact, metadata: { ...metadata, signature: { ...metadata.signature, digest: "sha256:" + "f".repeat(64) }, }, expectedPackage: metadata.packageName, expectedPlatform: metadata.platform, verifySignature: async () => true }), ArtifactVerificationError);
});
