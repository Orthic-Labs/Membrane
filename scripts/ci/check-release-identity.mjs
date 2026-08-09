#!/usr/bin/env node
import assert from "node:assert/strict";
import { fileURLToPath } from "node:url";
import { engineReleaseIdentity } from "../../apps/membrane-hub/scripts/release-identity.mjs";

const repoRoot = fileURLToPath(new URL("../..", import.meta.url));
const identity = engineReleaseIdentity(repoRoot);

assert.match(identity.sourceTreeSha256, /^[0-9a-f]{64}$/, "source tree digest must be SHA-256");
assert.equal(identity.releaseGeneration, `sha256:${identity.sourceTreeSha256}`, "release generation must derive from source digest");
assert.ok(identity.fileCount > 0, "engine source tree must contain files");
assert.equal(identity.dirty, false, "release identity requires a clean engine tree");

console.log(`release identity OK: ${identity.releaseGeneration} files=${identity.fileCount}`);
