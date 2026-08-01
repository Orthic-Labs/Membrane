import assert from "node:assert/strict";
import { realpath } from "node:fs/promises";
import test from "node:test";
import { buildRepositoryCatalog, catalogDigest, enrollRepositoryCatalog, hasExplicitChildGrant } from "./repository-catalog.mjs";
import { readRegistry } from "./project-registry.mjs";

const workspace = await realpath(new URL("../../", import.meta.url));

test("repository catalog discovers the workspace root plus all registered child repositories", async () => {
  const catalog = await buildRepositoryCatalog(workspace);
  assert.equal(catalog.schema, "orthic.repository-catalog.v1");
  assert.equal(catalog.repositories.length, 19);
  assert.equal(catalog.repositories.filter((entry) => entry.role === "workspace-root").length, 1);
  assert.equal(catalog.repositories.filter((entry) => entry.role === "child-repository").length, 18);
  assert.equal(catalog.catalog_digest, catalogDigest(catalog));
  assert.equal(new Set(catalog.repositories.map((entry) => entry.repository_id)).size, 19);
  assert.equal(new Set(catalog.repositories.map((entry) => entry.scope_id)).size, 19);
});

test("child graph access requires an explicit root grant", async () => {
  const catalog = await buildRepositoryCatalog(workspace);
  const root = catalog.repositories.find((entry) => entry.role === "workspace-root");
  const child = catalog.repositories.find((entry) => entry.role === "child-repository");
  assert.equal(hasExplicitChildGrant(catalog, root.repository_id, child.repository_id), false);
  assert.equal(hasExplicitChildGrant(catalog, root.repository_id, child.repository_id, [child.repository_id]), true);
  assert.equal(hasExplicitChildGrant(catalog, child.repository_id, root.repository_id, [child.repository_id]), false);
});

test("catalog enrollment binds every repository to one digest without implicit child grants", async () => {
  const temporary = await import("node:fs/promises").then(({ mkdtemp }) => mkdtemp("/tmp/membrane-catalog-"));
  const registry = `${temporary}/registry.json`;
  const result = await enrollRepositoryCatalog(workspace, { registryPath: registry, dryRun: true });
  assert.equal(result.bindings.length, 19);
  assert.ok(result.bindings.every((binding) => binding.repository_catalog_digest === result.catalog.catalog_digest));
  assert.deepEqual(result.bindings.find((binding) => binding.repository_id === result.catalog.repositories.find((entry) => entry.role === "workspace-root").repository_id).grant_policy.child_repository_ids, []);
  const applied = await enrollRepositoryCatalog(workspace, { registryPath: registry, childGrants: result.catalog.repositories.filter((entry) => entry.role === "child-repository").map((entry) => entry.repository_id) });
  const stored = await readRegistry(registry);
  assert.equal(applied.bindings.length, 19);
  assert.equal(Object.keys(stored.bindings).length, 19);
  assert.ok(Object.values(stored.bindings).every((binding) => binding.repository_catalog_digest === applied.catalog.catalog_digest));
});
