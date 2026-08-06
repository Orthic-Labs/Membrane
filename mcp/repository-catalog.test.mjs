import assert from "node:assert/strict";
import { mkdtemp, mkdir, realpath, rm } from "node:fs/promises";
import { tmpdir } from "node:os";
import { join } from "node:path";
import test from "node:test";
import { buildRepositoryCatalog, catalogDigest, enrollRepositoryCatalog, hasExplicitChildGrant } from "./repository-catalog.mjs";
import { readRegistry } from "./project-registry.mjs";

// MBR-015: the unit tests below run against a SYNTHETIC fixture workspace so a
// clean checkout of Membrane alone can build and run them — they never assume
// the real 20-repo workspace (or any sibling source tree) is present.

async function fixtureWorkspace(childNames) {
  const dir = await mkdtemp(join(tmpdir(), "membrane-catalog-"));
  const root = await realpath(dir);
  // Each child is a directory with a `.git` marker so discovery treats it as a
  // repo; no graph.db is needed for the discovery/grant/enrollment assertions.
  for (const name of childNames) {
    await mkdir(join(root, name, ".git"), { recursive: true });
  }
  return { root, cleanup: () => rm(root, { recursive: true, force: true }) };
}

const CHILDREN = ["heardright", "coderight", "mailright"];
const EXPECTED_TOTAL = CHILDREN.length + 1; // workspace root + children

test("catalog discovers the workspace root plus every child repository (fixture, sibling-independent)", async () => {
  const { root, cleanup } = await fixtureWorkspace(CHILDREN);
  const catalog = await buildRepositoryCatalog(root);
  assert.equal(catalog.schema, "orthic.repository-catalog.v1");
  assert.equal(catalog.repositories.length, EXPECTED_TOTAL);
  assert.equal(catalog.repositories.filter((entry) => entry.role === "workspace-root").length, 1);
  assert.equal(catalog.repositories.filter((entry) => entry.role === "child-repository").length, CHILDREN.length);
  assert.equal(catalog.catalog_digest, catalogDigest(catalog));
  assert.equal(new Set(catalog.repositories.map((entry) => entry.repository_id)).size, EXPECTED_TOTAL);
  assert.equal(new Set(catalog.repositories.map((entry) => entry.scope_id)).size, EXPECTED_TOTAL);
  await cleanup();
});
test("child graph access requires an explicit root grant (fixture)", async () => {
  const { root, cleanup } = await fixtureWorkspace(CHILDREN);
  const catalog = await buildRepositoryCatalog(root);
  const rootEntry = catalog.repositories.find((entry) => entry.role === "workspace-root");
  const child = catalog.repositories.find((entry) => entry.role === "child-repository");
  assert.equal(hasExplicitChildGrant(catalog, rootEntry.repository_id, child.repository_id), false);
  assert.equal(hasExplicitChildGrant(catalog, rootEntry.repository_id, child.repository_id, [child.repository_id]), true);
  assert.equal(hasExplicitChildGrant(catalog, child.repository_id, rootEntry.repository_id, [child.repository_id]), false);
  await cleanup();
});

test("catalog enrollment binds every repository to one digest without implicit child grants (fixture)", async () => {
  const { root, cleanup } = await fixtureWorkspace(CHILDREN);
  const temporary = await mkdtemp(join(tmpdir(), "membrane-catalog-reg-"));
  const registry = `${temporary}/registry.json`;
  const result = await enrollRepositoryCatalog(root, { registryPath: registry, dryRun: true });
  assert.equal(result.bindings.length, EXPECTED_TOTAL);
  assert.ok(result.bindings.every((binding) => binding.repository_catalog_digest === result.catalog.catalog_digest));
  const rootId = result.catalog.repositories.find((entry) => entry.role === "workspace-root").repository_id;
  assert.deepEqual(result.bindings.find((binding) => binding.repository_id === rootId).grant_policy.child_repository_ids, []);
  const applied = await enrollRepositoryCatalog(root, { registryPath: registry, childGrants: result.catalog.repositories.filter((entry) => entry.role === "child-repository").map((entry) => entry.repository_id) });
  const stored = await readRegistry(registry);
  assert.equal(applied.bindings.length, EXPECTED_TOTAL);
  assert.equal(Object.keys(stored.bindings).length, EXPECTED_TOTAL);
  assert.ok(Object.values(stored.bindings).every((binding) => binding.repository_catalog_digest === applied.catalog.catalog_digest));
  await rm(temporary, { recursive: true, force: true });
  await cleanup();
});

// Ambient workspace integration: when the real multi-repo workspace IS present
// (the resolved parent holds the sibling repos, e.g. cortex/ and forge/), the
// catalog must surface the full federation. From a clean checkout of Membrane
// alone the parent holds only this repo, so the assertion degrades to
// membrane-alone — proving the runtime never hard-requires a sibling source
// tree (MBR-015). The real workspace now has 20 repos (tools/skills/nemesis
// became a submodule); MBR-009 set that count.
test("ambient workspace discovery is sibling-independent and reports the real count when present", async () => {
  const ambient = await realpath(new URL("../../", import.meta.url));
  const catalog = await buildRepositoryCatalog(ambient);
  const childCount = catalog.repositories.filter((entry) => entry.role === "child-repository").length;
  assert.equal(catalog.repositories.filter((entry) => entry.role === "workspace-root").length, 1);
  if (childCount >= 19) {
    // Full workspace present: 20 repos total (19 children + workspace root).
    assert.equal(catalog.repositories.length, 20);
    assert.equal(childCount, 19);
  } else {
    // Clean checkout of Membrane alone: only this repo is discoverable, and the
    // catalog must still be self-consistent (stable digest, unique identities).
    assert.equal(catalog.catalog_digest, catalogDigest(catalog));
    assert.equal(new Set(catalog.repositories.map((entry) => entry.repository_id)).size, catalog.repositories.length);
  }
});
