// D09: package metadata test — exact product IDs and matching version fields.

import assert from "node:assert/strict";
import { readFileSync } from "node:fs";
import { join } from "node:path";
import test from "node:test";

const ROOT = join(import.meta.dirname, "..");
const pkg = JSON.parse(readFileSync(join(ROOT, "package.json"), "utf8"));

test("package identity fields are consistent", () => {
  assert.equal(pkg.name, "@membrane/blueprint");
  assert.equal(pkg.version, "0.2.0");
  assert.equal(pkg.repository.url, "git+https://github.com/Orthic-Labs/Membrane.git");
  assert.equal(pkg.repository.directory, "blueprint");
  assert.equal(pkg.homepage, "https://github.com/Orthic-Labs/Membrane/tree/main/blueprint#readme");
  assert.equal(pkg.bugs.url, "https://github.com/Orthic-Labs/Membrane/issues");
  assert.equal(pkg.mcpName, "io.github.Membrane/blueprint");
});

test("all required bins are declared", () => {
  for (const bin of ["blueprint", "blueprint-watch", "blueprint-mcp", "blueprint-install"]) {
    assert.ok(pkg.bin?.[bin], `missing bin ${bin}`);
  }
});

test("exports include schemas, contracts, and service", () => {
  assert.equal(pkg.exports["./schemas"], "./schemas/catalog.json");
  assert.equal(pkg.exports["./contracts"], "./src/lib/contracts/validate.mjs");
  assert.equal(pkg.exports["./service"], "./src/lib/application/service.mjs");
});

test("exports include the sdk entry point with types", () => {
  assert.equal(pkg.exports["./sdk"].types, "./src/sdk/types.d.ts");
  assert.equal(pkg.exports["./sdk"].default, "./src/sdk/index.mjs");
});

test("files allowlist ships sdk and service directories", () => {
  assert.ok(pkg.files.includes("src/sdk/"), "files must include src/sdk/");
  assert.ok(pkg.files.includes("src/service/"), "files must include src/service/");
});

test("publish config is public and sideEffects false", () => {
  assert.equal(pkg.publishConfig?.access, "public");
  assert.equal(pkg.sideEffects, false);
});

test("engines match the branded >=22.22.3 requirement", () => {
  assert.equal(pkg.engines.node, ">=22.22.3");
});
