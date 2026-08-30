import assert from "node:assert/strict";
import { readFileSync } from "node:fs";
import { join } from "node:path";
import test from "node:test";
import { fileURLToPath } from "node:url";

const root = fileURLToPath(new URL("../..", import.meta.url));
const read = path => readFileSync(join(root, path), "utf8");
const json = path => JSON.parse(read(path));
const platformKeys = ["win32-x64"];

test("generated npm package graph stays Membrane-owned", () => {
  const npm = json("dist/npm/package.json");
  assert.equal(npm.name, "@membrane/membrane");
  assert.deepEqual(Object.keys(npm.optionalDependencies).sort(), platformKeys.map(key => `@membrane/membrane-${key}`).sort());
  const index = read("dist/npm/index.mjs");
  for (const key of platformKeys) assert.match(index, new RegExp(`"${key}": "@membrane/membrane-${key}"`));
  for (const key of platformKeys) assert.equal(json(`dist/npm/platforms/${key}/package.json`).name, `@membrane/membrane-${key}`);
  assert.equal(json("dist/packages/typescript/package.json").name, "@membrane/membrane-client");
});

test("generated OCI source contract uses current Membrane coordinate", () => {
  assert.match(read("dist/packaging/oci/Containerfile"), /^FROM ghcr\.io\/membrane\/membrane-runtime@sha256:[0-9a-f]{64}$/m);
  assert.doesNotMatch(read("dist/packaging/oci/Containerfile"), /ghcr\.io\/orthic\//i);
});

test("active install docs do not resurrect retired package or installer authority", () => {
  const docs = ["docs/README.md", "docs/product/installation/install.md", "docs/product/installation/npm.md", "docs/product/installation/oci.md", "docs/product/installation/registry.md", "docs/product/installation/README.md", "docs/architecture/runtime/tray-daemon-contract.md", "docs/product/getting-started.md", "docs/product/compatibility/release-channels.md", "docs/pending/README.md", "scripts/tools/productization/README.md"].map(read).join("\n");
  assert.doesNotMatch(docs, /@orthic\/membrane|orthic\.membrane|Orthic owns desktop installation|Orthic desktop installer/i);
  assert.match(docs, /Membrane Hub/);
});
