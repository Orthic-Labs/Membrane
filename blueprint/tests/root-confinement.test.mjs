// D06: root confinement — arbitrary model-supplied paths cannot select local
// directories when the service runs in registry mode.

import assert from "node:assert/strict";
import { cpSync, mkdtempSync, rmSync, symlinkSync, mkdirSync } from "node:fs";
import { tmpdir } from "node:os";
import { join } from "node:path";
import test from "node:test";

import { createBlueprintApplicationService } from "../src/lib/application/service.mjs";
import { RootRegistry } from "../src/lib/application/root-registry.mjs";
import { BlueprintError } from "../src/lib/application/errors.mjs";
import { buildGraphGeneration } from "../src/graph/static-provider.mjs";

const FIXTURE = join(import.meta.dirname, "..", "evals/fixture-repos/typescript-commerce");

function enrolledService() {
  const root = mkdtempSync(join(tmpdir(), "blueprint-confine-"));
  cpSync(FIXTURE, root, { recursive: true });
  buildGraphGeneration(root, { outDir: ".agent", persist: true });
  const registry = new RootRegistry([{ root, repoId: "repo-1" }]);
  const service = createBlueprintApplicationService({ rootRegistry: registry, allowEmbeddedRoot: false });
  return { root, service };
}

async function expectRootError(promise, code) {
  try {
    await promise;
  } catch (error) {
    assert.ok(error instanceof BlueprintError, `expected BlueprintError, got ${error}`);
    assert.equal(error.code, code);
    return error;
  }
  assert.fail(`expected root error ${code}`);
}

test("registry mode rejects an arbitrary repoRoot", async () => {
  const { root, service } = enrolledService();
  const elsewhere = mkdtempSync(join(tmpdir(), "blueprint-elsewhere-"));
  try {
    const error = await expectRootError(service.search({ repoRoot: elsewhere, query: "placeOrder" }), "root_not_enrolled");
    // No directory listing leaks in the error.
    assert.ok(!JSON.stringify(error).includes("node_modules"));
  } finally {
    rmSync(root, { recursive: true, force: true });
    rmSync(elsewhere, { recursive: true, force: true });
  }
});

test("registry mode resolves by enrolled repoId", async () => {
  const { root, service } = enrolledService();
  try {
    const result = await service.search({ repoId: "repo-1", query: "placeOrder", limit: 5 });
    assert.ok(Array.isArray(result.results));
  } finally {
    rmSync(root, { recursive: true, force: true });
  }
});

test("repoId with mismatched root fails closed", async () => {
  const { root, service } = enrolledService();
  const elsewhere = mkdtempSync(join(tmpdir(), "blueprint-elsewhere2-"));
  try {
    try {
      await service.search({ repoId: "repo-1", repoRoot: elsewhere, query: "placeOrder" });
    } catch (error) {
      assert.ok(error instanceof BlueprintError, `expected BlueprintError, got ${error}`);
      assert.ok(error.code === "root_escape" || error.code === "root_not_enrolled", `got ${error.code}`);
      return;
    }
    assert.fail("expected a root confinement error");
  } finally {
    rmSync(root, { recursive: true, force: true });
    rmSync(elsewhere, { recursive: true, force: true });
  }
});

test("symlink escape never resolves to the linked directory", async () => {
  const { root, service } = enrolledService();
  const outside = mkdtempSync(join(tmpdir(), "blueprint-outside-"));
  mkdirSync(join(outside, ".git"), { recursive: true });
  try {
    const link = join(root, "confine-link");
    symlinkSync(outside, link);
    await expectRootError(service.search({ repoRoot: link, query: "placeOrder" }), "root_not_enrolled");
  } finally {
    rmSync(root, { recursive: true, force: true });
    rmSync(outside, { recursive: true, force: true });
  }
});

test("embedded mode still accepts an explicit repoRoot", async () => {
  const root = mkdtempSync(join(tmpdir(), "blueprint-embedded-"));
  cpSync(FIXTURE, root, { recursive: true });
  buildGraphGeneration(root, { outDir: ".agent", persist: true });
  try {
    const service = createBlueprintApplicationService({ allowEmbeddedRoot: true });
    const result = await service.search({ repoRoot: root, query: "placeOrder", limit: 5 });
    assert.ok(Array.isArray(result.results));
  } finally {
    rmSync(root, { recursive: true, force: true });
  }
});

test("registry mode without any matching repo raises root_not_enrolled", async () => {
  const { service } = enrolledService();
  const nowhere = mkdtempSync(join(tmpdir(), "blueprint-nowhere-"));
  try {
    await expectRootError(service.status({ repoRoot: nowhere }), "root_not_enrolled");
  } finally {
    rmSync(nowhere, { recursive: true, force: true });
  }
});
