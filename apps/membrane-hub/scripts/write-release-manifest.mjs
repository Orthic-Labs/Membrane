// Sync the workspace release manifest with the identity the engine was just
// built against, so `expected` (manifest) and `observed` (baked into the binary)
// agree and the gateway fans out instead of degrading.
//
// Scope is deliberately narrow: only the two top-level fields the federation
// gateway validates (`release_generation`, `source_tree_sha256`, which it
// requires to be internally consistent — see `_expected_release_generation`).
// Per-asset hashes are a distribution seal covering binaries for every platform;
// a local macOS build cannot honestly restamp the Windows entries, so this
// leaves them alone and says so. Sealing a shippable manifest stays the job of
// `right-release`.
//
// Usage: node scripts/write-release-manifest.mjs [--manifest <path>] [--check]

import { readFileSync, writeFileSync } from "node:fs";
import { engineReleaseIdentity } from "./release-identity.mjs";

const argv = process.argv.slice(2);
const checkOnly = argv.includes("--check");
const manifestFlag = argv.indexOf("--manifest");
const manifestPath = manifestFlag !== -1
  ? argv[manifestFlag + 1]
  : new URL("../../../../tools/lib/crypt-release.json", import.meta.url).pathname;

// Prefer the identity recorded by the build over a fresh computation. The two
// differ whenever the engine tree changed while cargo was running, and it is the
// baked value — not today's tree — that the running binary reports back to the
// gateway. Recomputing here would write a manifest that never matches.
const recorded = new URL("../dist/release-identity.json", import.meta.url);
const repoRoot = new URL("../../../", import.meta.url).pathname.replace(/\/$/, "");
let identity;
try {
  identity = JSON.parse(readFileSync(recorded, "utf8"));
} catch {
  identity = engineReleaseIdentity(repoRoot);
}

const document = JSON.parse(readFileSync(manifestPath, "utf8"));
const current = document.release_generation;
const matches = current === identity.releaseGeneration;

if (checkOnly) {
  console.log(`manifest : ${current}`);
  console.log(`engine   : ${identity.releaseGeneration}`);
  console.log(matches ? "MATCHED" : "MISMATCH — run without --check to sync");
  process.exit(matches ? 0 : 1);
}

if (matches) {
  console.log(`[membrane] release manifest already at ${current}`);
  process.exit(0);
}

document.crypt_source_commit = identity.commit;
document.source_tree_path = identity.sourceTreePath;
document.source_tree_sha256 = identity.sourceTreeSha256;
document.release_generation = identity.releaseGeneration;
writeFileSync(manifestPath, `${JSON.stringify(document, null, 2)}\n`);

const staleAssets = (document.assets ?? []).filter(
  (asset) => asset.release_generation && asset.release_generation !== identity.releaseGeneration,
);
console.log(`[membrane] release manifest ${current} -> ${identity.releaseGeneration}`);
if (staleAssets.length > 0) {
  const names = staleAssets.map((asset) => `${asset.os}/${asset.name}`).join(", ");
  console.log(`[membrane] asset seals NOT restamped (reseal via right-release): ${names}`);
}
