import { createHash } from "node:crypto";

/** Fixture/template registry. Entries become real package releases only after registry approval. */
export const PLATFORM_PACKAGES = Object.freeze({
  "darwin-arm64": "@orthic/membrane-darwin-arm64",
  "darwin-x64": "@orthic/membrane-darwin-x64",
  "linux-arm64": "@orthic/membrane-linux-arm64",
  "linux-x64": "@orthic/membrane-linux-x64",
  "win32-arm64": "@orthic/membrane-win32-arm64",
  "win32-x64": "@orthic/membrane-win32-x64"
});

const digest = bytes => `sha256:${createHash("sha256").update(bytes).digest("hex")}`;
const fail = (message, ErrorType = Error) => { throw new ErrorType(message); };

export class UnsupportedPlatformError extends Error {}
export class ArtifactVerificationError extends Error {}

export function platformKey(platform = process.platform, arch = process.arch) {
  return `${platform}-${arch}`;
}

export function selectPlatform({ platform = process.platform, arch = process.arch, packages = PLATFORM_PACKAGES } = {}) {
  const key = platformKey(platform, arch);
  const packageName = packages[key];
  if (!packageName) fail(`unsupported Membrane platform: ${key}`, UnsupportedPlatformError);
  return { key, packageName };
}

function signatureMetadata(metadata, expectedDigest, expectedPackage, expectedPlatform) {
  const signature = metadata?.signature;
  if (!signature || typeof signature !== "object" ||
      signature.algorithm !== "ed25519" || typeof signature.keyId !== "string" || !signature.keyId.trim() ||
      typeof signature.value !== "string" || !signature.value.trim() || signature.digest !== expectedDigest ||
      metadata.packageName !== expectedPackage || metadata.platform !== expectedPlatform || typeof metadata.version !== "string" || !metadata.version.trim()) {
    fail("native artifact signature metadata is missing or does not bind to SHA-256", ArtifactVerificationError);
  }
  return signature;
}

/** Validate bytes, digest, and signed metadata before any native dispatch. */
export async function verifyArtifact({ artifact, metadata, verifySignature, expectedPackage, expectedPlatform } = {}) {
  if (!(artifact instanceof Uint8Array) && typeof artifact !== "string") {
    fail("native artifact must be bytes or a string", ArtifactVerificationError);
  }
  const actualDigest = digest(artifact);
  if (metadata?.sha256 !== actualDigest) {
    fail(`native artifact SHA-256 mismatch: expected ${metadata?.sha256 ?? "missing"}, got ${actualDigest}`, ArtifactVerificationError);
  }
  const signature = signatureMetadata(metadata, actualDigest, expectedPackage, expectedPlatform);
  if (typeof verifySignature !== "function" || !(await verifySignature({ artifact, digest: actualDigest, signature, packageName: expectedPackage, platform: expectedPlatform, version: metadata.version }))) {
    fail("native artifact signature verification failed", ArtifactVerificationError);
  }
  return { digest: actualDigest, signature };
}

/** Resolve, verify, then invoke a native package's dispatch function. */
export async function bootstrap({ artifact, metadata, load = async name => import(name), verifySignature, ...platform } = {}) {
  const target = selectPlatform(platform);
  const verification = await verifyArtifact({ artifact, metadata, verifySignature, expectedPackage: target.packageName, expectedPlatform: target.key });
  const native = await load(target.packageName);
  if (!native || typeof native.dispatch !== "function") fail(`native package ${target.packageName} has no dispatch function`, ArtifactVerificationError);
  return { target, verification, dispatch: (...args) => native.dispatch(...args) };
}

export { digest as artifactDigest };
