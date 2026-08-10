import assert from "node:assert/strict";
import { readFileSync, existsSync } from "node:fs";
import { join, dirname } from "node:path";
import { fileURLToPath } from "node:url";
import test from "node:test";

const HERE = dirname(fileURLToPath(import.meta.url));
const ROOT = join(HERE, "..");
const pkg = JSON.parse(readFileSync(join(ROOT, "package.json"), "utf8"));

test("package.json exposes standalone version, bin, engines, and files", () => {
  assert.equal(pkg.name, "@orthic-labs/cortex");
  assert.equal(pkg.bin?.cortex, "./scripts/cortex.mjs");
  assert.match(String(pkg.version), /^\d+\.\d+\.\d+/);
  assert.equal(pkg.type, "module");
  assert.ok(pkg.engines?.node);
  assert.match(pkg.engines.node, />=22\.22\.3/);
  assert.ok(existsSync(join(ROOT, "scripts/cortex.mjs")));
  assert.ok(Array.isArray(pkg.files));
  for (const required of ["scripts/", "graph/", "lib/", "sources/", "schemas/"]) {
    assert.ok(pkg.files.includes(required), `files must include ${required}`);
  }
  assert.equal(pkg.exports?.["./admission"], "./lib/admission.mjs");
  assert.ok(pkg.scripts?.test);
  assert.ok(pkg.scripts?.["test:workspace"]);
  assert.doesNotMatch(pkg.scripts.test, /workspace/);
  assert.match(pkg.scripts["test:workspace"], /workspace/);
});

test("admission library and schemas ship in the package surface", () => {
  assert.ok(existsSync(join(ROOT, "lib/admission.mjs")));
  assert.ok(existsSync(join(ROOT, "lib/receipt-store.mjs")));
  assert.ok(existsSync(join(ROOT, "lib/orientation-evidence.mjs")));
  assert.ok(existsSync(join(ROOT, "schemas/cortex-admission-v1.schema.json")));
});
