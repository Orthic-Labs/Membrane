import assert from "node:assert/strict";
import { readFile } from "node:fs/promises";
import { dirname, resolve } from "node:path";
import { fileURLToPath } from "node:url";

const root = resolve(dirname(fileURLToPath(import.meta.url)), "../../..");
const manifest = JSON.parse(await readFile(resolve(root, "dist/packaging/legal/manifest.json"), "utf8"));
const packageJson = JSON.parse(await readFile(resolve(root, "package.json"), "utf8"));
const documentKinds = new Set(["eula", "privacy", "third-party-notices"]);
const expectedPaths = new Set(["docs/legal/EULA.txt", "docs/legal/PRIVACY.md", "docs/legal/THIRD-PARTY-NOTICES.txt"]);

assert.equal(manifest.schemaVersion, 1);
assert.equal(manifest.product, "Membrane");
assert.equal(manifest.controllingLicense.title, "Orthic Labs Source Use License v1.0");
assert.equal(manifest.controllingLicense.path, "LICENSE");
assert.equal(packageJson.license, manifest.packageRegistry.license);
assert.deepEqual(
  manifest.packageDocuments.map(({ kind }) => kind).sort(),
  [...documentKinds].sort(),
  "package manifest must bind each required legal document exactly once",
);
assert.equal(new Set(manifest.packageDocuments.map(({ path }) => path)).size, documentKinds.size);
for (const document of manifest.packageDocuments) {
  assert.ok(documentKinds.has(document.kind), `unknown legal document kind: ${document.kind}`);
  assert.ok(expectedPaths.has(document.path), `unexpected legal document path: ${document.path}`);
  const content = await readFile(resolve(root, document.path), "utf8");
  assert.ok(content.trim(), `${document.kind} document must not be empty`);
}
assert.equal(manifest.distributionBinding.status, "unconfigured");
assert.equal(manifest.distributionBinding.requiredForRelease, true);

const eula = await readFile(resolve(root, "docs/legal/EULA.txt"), "utf8");
const privacy = await readFile(resolve(root, "docs/legal/PRIVACY.md"), "utf8");
const notices = await readFile(resolve(root, "docs/legal/THIRD-PARTY-NOTICES.txt"), "utf8");
assert.match(eula, /Orthic Labs Source Use License v1\.0/);
assert.match(privacy, /persists?[^.]{0,80}scope-grant[^.]{0,80}(token|digest)/i);

assert.equal(manifest.thirdPartyInventory.scope, "locked-javascript-and-rust-resolutions");
const pnpmLock = await readFile(resolve(root, manifest.thirdPartyInventory.pnpmLockfile), "utf8");
const cargoLock = await readFile(resolve(root, manifest.thirdPartyInventory.cargoLockfile), "utf8");
const jsPackages = [...pnpmLock.matchAll(/^  '?(.*?)'?:\n    resolution:/gm)].map((match) => match[1]);
const cargoPackages = cargoLock
  .split("[[package]]\n")
  .slice(1)
  .filter((entry) => /^source = "(?:registry\+|git\+)/m.test(entry));
assert.ok(jsPackages.length > 0, "pnpm lockfile must contain resolved packages");
assert.ok(cargoPackages.length > 0, "Cargo lockfile must contain registry or git packages");
assert.match(notices, /pnpm-lock\.yaml/);
assert.match(notices, /engine\/Cargo\.lock/);
assert.match(notices, /complete locked JavaScript resolution/i);
assert.match(notices, /complete locked Rust registry and git resolution/i);

process.stdout.write(
  `legal source inventory verified: ${jsPackages.length} JavaScript, ${cargoPackages.length} Rust; distribution binding unconfigured\n`,
);
