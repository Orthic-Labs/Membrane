import test from "node:test";
import assert from "node:assert/strict";
import { artifactDigest, UnsupportedPlatformError, ArtifactVerificationError } from "../../dist/npm/index.mjs";
import { runInstall, PlatformPackageMissingError, SigningTrustNotConfiguredError } from "../../dist/npm/bin/membrane.mjs";

const artifact = Buffer.from("native-fixture-cli");
const metadata = {
  sha256: artifactDigest(artifact),
  packageName: "@orthic/membrane-linux-x64",
  platform: "linux-x64",
  version: "0.1.0",
  signature: { algorithm: "ed25519", keyId: "fixture-key", value: "fixture-signature", digest: artifactDigest(artifact) },
};

function nativeFixture(overrides = {}) {
  return { artifact, metadata, dispatch: (value) => `dispatched:${value}`, ...overrides };
}

test("runInstall verifies and dispatches the co-installed platform package", async () => {
  const calls = [];
  const logs = [];
  const result = await runInstall({
    platform: "linux",
    arch: "x64",
    load: async (name) => { calls.push(name); return nativeFixture(); },
    verifySignature: async () => true,
    log: (message) => logs.push(message),
  });
  assert.equal(result.dispatch("ok"), "dispatched:ok");
  // The platform package is imported exactly once, then reused for bootstrap()'s own load.
  assert.deepEqual(calls, ["@orthic/membrane-linux-x64"]);
  assert.equal(logs.length, 1);
  assert.match(logs[0], /@orthic\/membrane-linux-x64/);
});

test("unsupported host tuples fail closed before any package is loaded", async () => {
  let loaded = false;
  await assert.rejects(
    () => runInstall({ platform: "freebsd", arch: "x64", load: async () => { loaded = true; }, verifySignature: async () => true }),
    UnsupportedPlatformError,
  );
  assert.equal(loaded, false);
});

test("a missing optional platform package is a declared gap, not a crash", async () => {
  await assert.rejects(
    () => runInstall({
      platform: "linux", arch: "x64",
      load: async () => { throw new Error("Cannot find package '@orthic/membrane-linux-x64'"); },
      verifySignature: async () => true,
    }),
    (error) => error instanceof PlatformPackageMissingError && /@orthic\/membrane-linux-x64/.test(error.message),
  );
});

test("a platform package that exports no artifact/metadata is a declared gap", async () => {
  await assert.rejects(
    () => runInstall({ platform: "linux", arch: "x64", load: async () => ({ dispatch() {} }), verifySignature: async () => true }),
    PlatformPackageMissingError,
  );
});

test("checksum mismatch is rejected before dispatch", async () => {
  let dispatched = false;
  await assert.rejects(
    () => runInstall({
      platform: "linux", arch: "x64",
      load: async () => nativeFixture({ metadata: { ...metadata, sha256: `sha256:${"0".repeat(64)}` }, dispatch: () => { dispatched = true; } }),
      verifySignature: async () => true,
    }),
    ArtifactVerificationError,
  );
  assert.equal(dispatched, false);
});

test("signature mismatch is rejected before dispatch", async () => {
  await assert.rejects(
    () => runInstall({
      platform: "linux", arch: "x64",
      load: async () => nativeFixture(),
      verifySignature: async () => false,
    }),
    ArtifactVerificationError,
  );
});

test("without an injected trust root, the default verifySignature fails closed instead of fabricating trust", async () => {
  await assert.rejects(
    () => runInstall({ platform: "linux", arch: "x64", load: async () => nativeFixture() }),
    SigningTrustNotConfiguredError,
  );
});
