// Usage: node scripts/write-release-manifest.mjs [--manifest <path>]
//   [--identity <path>] [--check]

import { readFileSync, writeFileSync } from "node:fs";
import { resolve } from "node:path";
import { execFileSync } from "node:child_process";
import { engineReleaseIdentity } from "./release-identity.mjs";
const args = process.argv.slice(2);
const options = { check: false };
for (let index = 0; index < args.length; index += 1) {
  const arg = args[index];
  if (arg === "--check") {
    if (options.check) throw new Error("--check may only be supplied once");
    options.check = true;
  } else if (arg === "--manifest" || arg === "--identity") {
    if (options[arg.slice(2)]) throw new Error(`${arg} may only be supplied once`);
    const value = args[++index];
    if (!value || value.startsWith("--")) throw new Error(`${arg} requires a path`);
    options[arg.slice(2)] = resolve(value);
  } else {
    throw new Error(`unknown argument: ${arg}`);
  }
}

const repoRoot = new URL("../../../", import.meta.url).pathname.replace(/\/$/, "");
const manifestPath = options.manifest
  ?? new URL("../../../../tools/lib/crypt-release.json", import.meta.url).pathname;
const identity = options.identity
  ? validatedRecordedIdentity(options.identity, repoRoot)
  : engineReleaseIdentity(repoRoot);
const document = JSON.parse(readFileSync(manifestPath, "utf8"));
const fields = ["crypt_source_commit", "source_tree_path", "source_tree_sha256", "release_generation"];
const expected = {
  crypt_source_commit: identity.commit,
  source_tree_path: identity.sourceTreePath,
  source_tree_sha256: identity.sourceTreeSha256,
  release_generation: identity.releaseGeneration,
};
const mismatched = fields.filter((field) => document[field] !== expected[field]);
if (options.check) {
  if (mismatched.length) {
    throw new Error(`manifest identity mismatch: ${mismatched.join(", ")}`);
  }
  console.log(`[membrane] release manifest matches ${identity.releaseGeneration}`);
  process.exit(0);
}
if (!mismatched.length) {
  console.log(`[membrane] release manifest already at ${identity.releaseGeneration}`);
  process.exit(0);
}
Object.assign(document, expected);
writeFileSync(manifestPath, `${JSON.stringify(document, null, 2)}\n`);
console.log(`[membrane] release manifest updated: ${mismatched.join(", ")}`);

function validatedRecordedIdentity(path, root) {
  const identity = JSON.parse(readFileSync(path, "utf8"));
  const keys = ["commit", "dirty", "fileCount", "sourceTreePath", "sourceTreeSha256", "releaseGeneration"];
  if (Object.keys(identity).length !== keys.length || keys.some((key) => !(key in identity))) {
    throw new Error("recorded identity has an invalid shape");
  }
  const head = execFileSync("git", ["-C", root, "rev-parse", "HEAD"], { encoding: "utf8" }).trim();
  if (identity.commit !== head || !/^[0-9a-f]{40}$/i.test(identity.commit)) {
    throw new Error("recorded identity commit is not current HEAD");
  }
  if (typeof identity.dirty !== "boolean" || !Number.isSafeInteger(identity.fileCount) || identity.fileCount < 0) {
    throw new Error("recorded identity has invalid build metadata");
  }
  if (identity.sourceTreePath !== "engine" || !/^[0-9a-f]{64}$/i.test(identity.sourceTreeSha256)) {
    throw new Error("recorded identity has invalid engine digest");
  }
  if (identity.releaseGeneration !== `sha256:${identity.sourceTreeSha256}`) {
    throw new Error("recorded identity release generation does not match digest");
  }
  return identity;
}
