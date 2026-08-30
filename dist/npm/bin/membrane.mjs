#!/usr/bin/env node
// MBR-906: `npx @membrane/membrane install` -- the thin bootstrapper's only
// user-facing entry point. It never bundles or executes an unverified
// native artifact: it locates the optionalDependency platform package that
// `npm install` already selected for this host (via that package's own
// os/cpu fields), requires it to export the artifact bytes + signed
// metadata for the platform-specific native runtime, verifies both through
// npm/index.mjs's existing digest+signature checks, and only then exposes
// `dispatch`. No version, digest, or signature is invented here: every
// value comes from the co-installed platform package, or the run fails
// closed with a named, honest gap.
import { pathToFileURL } from "node:url";
import { resolve as resolvePath } from "node:path";
import { bootstrap, selectPlatform, UnsupportedPlatformError, ArtifactVerificationError } from "../index.mjs";

export class PlatformPackageMissingError extends Error {}
export class SigningTrustNotConfiguredError extends Error {}

// This npm package ships no signing public key: hardcoding one here would
// fabricate trust that release/signing tooling (MBR-901/MBR-902/MBR-911,
// outside this task's allowlist) actually owns and rotates. The default
// fails closed instead of silently accepting an unverifiable signature;
// callers with a real configured trust root pass their own
// `verifySignature` (see docs/product/installation/npm.md).
async function defaultVerifySignature() {
  throw new SigningTrustNotConfiguredError(
    "declared gap: no trusted Membrane signing key is configured in @membrane/membrane; artifact signature verification cannot proceed. Signing-key trust is a release gate (see docs/product/installation/npm.md), not something this package fabricates.",
  );
}

/**
 * Resolves, verifies, then dispatches into the native platform package
 * npm's own optionalDependency os/cpu matching already installed for this
 * host. Fails closed, with a distinct, named reason, at every stage:
 * unsupported host tuple (UnsupportedPlatformError), platform package not
 * installed (PlatformPackageMissingError), digest/signature mismatch
 * (ArtifactVerificationError), or unconfigured signing trust
 * (SigningTrustNotConfiguredError).
 */
export async function runInstall({ platform = process.platform, arch = process.arch, load, verifySignature = defaultVerifySignature, log = () => {} } = {}) {
  const loader = load ?? ((name) => import(name));
  const target = selectPlatform({ platform, arch });

  let native;
  try {
    native = await loader(target.packageName);
  } catch (error) {
    throw new PlatformPackageMissingError(
      `declared gap: optional platform package ${target.packageName} is not installed for ${target.key}. Native artifact production and publication remain release gates (see docs/product/installation/npm.md). Underlying error: ${error.message}`,
    );
  }
  if (!native || typeof native !== "object" || !native.artifact || !native.metadata) {
    throw new PlatformPackageMissingError(
      `declared gap: platform package ${target.packageName} did not export a verified artifact/metadata pair yet for ${target.key}`,
    );
  }

  // Reuse the already-loaded package instance for bootstrap()'s own load,
  // instead of importing it a second time: same object, one real import.
  const cachedLoad = async () => native;
  const result = await bootstrap({ artifact: native.artifact, metadata: native.metadata, platform, arch, load: cachedLoad, verifySignature });
  log(`membrane: verified ${target.packageName} (${target.key}), digest ${result.verification.digest}`);
  return result;
}

function usage() {
  return "usage: membrane install\n\nVerifies and dispatches the Membrane native runtime already selected by\nnpm's optionalDependency platform resolution for this host. Never\ndownloads, bundles, or trusts an artifact that fails digest or signature\nverification.";
}

async function cli(argv) {
  const [command] = argv;
  if (command !== "install") {
    console.log(usage());
    process.exitCode = command ? 2 : 0;
    return;
  }
  try {
    await runInstall({ log: (message) => console.log(message) });
  } catch (error) {
    console.error(error.message);
    process.exitCode = 1;
  }
}

export { UnsupportedPlatformError, ArtifactVerificationError };

if (process.argv[1] && import.meta.url === pathToFileURL(resolvePath(process.argv[1])).href) cli(process.argv.slice(2));
