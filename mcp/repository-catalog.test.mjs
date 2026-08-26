import assert from "node:assert/strict";
import { mkdtemp, mkdir, realpath, rm, writeFile } from "node:fs/promises";
import { tmpdir } from "node:os";
import { join } from "node:path";
import test from "node:test";
import { buildRepositoryCatalog, catalogDigest, discoverRepositoryRoots, enrollRepositoryCatalog, hasExplicitChildGrant } from "./repository-catalog.mjs";
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
  assert.equal(catalog.schema, "membrane.repository-catalog.v1");
  assert.equal(catalog.repositories.length, EXPECTED_TOTAL);
  assert.equal(catalog.repositories.filter((entry) => entry.repoId === catalog.workspace_id).length, 1);
  assert.equal(catalog.repositories.filter((entry) => entry.repoId !== catalog.workspace_id).length, CHILDREN.length);
  assert.equal(catalog.catalog_digest, catalogDigest(catalog));
  assert.equal(new Set(catalog.repositories.map((entry) => entry.repoId)).size, EXPECTED_TOTAL);
  for (const entry of catalog.repositories) {
    assert.equal(entry.origin, null, "repository origin is Blueprint-owned and unavailable here");
    assert.equal(entry.sourceCommit, null, "repository HEAD is Blueprint-owned and unavailable here");
    assert.equal(entry.identityStatus, "unknown");
    assert.equal(entry.identityReason, "blueprint_repository_identity_unavailable");
  }
  for (const entry of catalog.repositories) for (const key of ["repository_id", "scope_id", "root", "role", "blueprint_graph", "grants"]) assert.equal(Object.hasOwn(entry, key), false, `retired catalog key: ${key}`);
  await cleanup();
});
test("catalog ignores .gitmodules paths without a live Git repository", async () => {
  const { root, cleanup } = await fixtureWorkspace([]);
  await writeFile(join(root, ".gitmodules"), "[submodule \"phantom\"]\n\tpath = phantom\n", "utf8");
  const discovered = await discoverRepositoryRoots(root);
  assert.deepEqual(discovered.map(({ relativeRoot }) => relativeRoot), ["."]);
  await cleanup();
});
test("child graph access requires an explicit root grant (fixture)", async () => {
  const { root, cleanup } = await fixtureWorkspace(CHILDREN);
  const catalog = await buildRepositoryCatalog(root);
  const rootEntry = catalog.repositories.find((entry) => entry.repoId === catalog.workspace_id);
  const child = catalog.repositories.find((entry) => entry.repoId !== catalog.workspace_id);
  assert.equal(hasExplicitChildGrant(catalog, rootEntry.repoId, child.repoId), false);
  assert.equal(hasExplicitChildGrant(catalog, rootEntry.repoId, child.repoId, [child.repoId]), true);
  assert.equal(hasExplicitChildGrant(catalog, child.repoId, rootEntry.repoId, [child.repoId]), false);
  await cleanup();
});

test("catalog enrollment binds every repository to one digest without implicit child grants (fixture)", async () => {
  const { root, cleanup } = await fixtureWorkspace(CHILDREN);
  const temporary = await mkdtemp(join(tmpdir(), "membrane-catalog-reg-"));
  const registry = `${temporary}/registry.json`;
  const result = await enrollRepositoryCatalog(root, { registryPath: registry, dryRun: true });
  assert.equal(result.bindings.length, EXPECTED_TOTAL);
  assert.ok(result.bindings.every((binding) => binding.repository_catalog_digest === result.catalog.catalog_digest));
  const rootId = result.catalog.repositories.find((entry) => entry.repoId === result.catalog.workspace_id).repoId;
  assert.deepEqual(result.bindings.find((binding) => binding.repository_id === rootId).grant_policy.child_repository_ids, []);
  const applied = await enrollRepositoryCatalog(root, { registryPath: registry, childGrants: result.catalog.repositories.filter((entry) => entry.repoId !== result.catalog.workspace_id).map((entry) => entry.repoId) });
  const stored = await readRegistry(registry);
  assert.equal(applied.bindings.length, EXPECTED_TOTAL);
  assert.equal(Object.keys(stored.bindings).length, EXPECTED_TOTAL);
  assert.ok(Object.values(stored.bindings).every((binding) => binding.repository_catalog_digest === applied.catalog.catalog_digest));
  await rm(temporary, { recursive: true, force: true });
  await cleanup();
});

// Ambient workspace integration: when the real multi-repo workspace IS present
// (the resolved parent holds the sibling repos, e.g. blueprint/ and forge/), the
// catalog must surface the full federation. From a clean checkout of Membrane
// alone the parent holds only this repo, so the assertion degrades to
// membrane-alone — proving the runtime never hard-requires a sibling source
// tree (MBR-015). Ambient workspaces may gain independent repos over time, so
// this integration asserts self-consistency rather than freezing machine state.
test("ambient workspace discovery is sibling-independent and reports the real count when present", async () => {
  const ambient = await realpath(new URL("../../", import.meta.url));
  const catalog = await buildRepositoryCatalog(ambient);
  const childCount = catalog.repositories.filter((entry) => entry.repoId !== catalog.workspace_id).length;
  assert.equal(catalog.repositories.filter((entry) => entry.repoId === catalog.workspace_id).length, 1);
  assert.equal(catalog.repositories.length, childCount + 1);
  assert.equal(catalog.catalog_digest, catalogDigest(catalog));
  assert.equal(new Set(catalog.repositories.map((entry) => entry.repoId)).size, catalog.repositories.length);
});
