#!/usr/bin/env node
// MBR-1001 — Self-verifying test for the docs/link gate.
//
// Book mode disallows adding suites under mcp/, so the checker's tests run as
// this directly runnable script. It proves both directions of the contract:
// the pure evaluators accept a consistent fixture and FAIL on deliberately
// broken ones (missing doc, stale doc, broken README link, wrong tool count).
//
// Run:  node scripts/tools/productization/check-docs.test.mjs
// Exit: 0 when all assertions pass, 1 otherwise.

import assert from "node:assert/strict";
import { evaluateGeneratedDocs, evaluateReadmeLinks, generatedDocSpecs } from "./check-docs.mjs";
import { computeProductTruth, computePlatformStatus } from "./generate-product-truth.mjs";
import { extractLocalLinks } from "./render-docs.mjs";

const checks = [];
function test(name, fn) {
  checks.push([name, fn]);
}

const truth = await computeProductTruth();
const platforms = await computePlatformStatus();
const specs = generatedDocSpecs(truth, platforms);
const current = new Map(specs.map((spec) => [spec.path, spec.render()]));

test("extractLocalLinks keeps local targets and drops URLs/anchors", () => {
  const links = extractLocalLinks(
    "[a](docs/product/README.md) [b](https://example.com/x) [c](#section) [d](docs/architecture/runtime-truth.md#flow) [e](mailto:x@y.z)",
  );
  assert.deepEqual(links, ["docs/architecture/runtime-truth.md", "docs/product/README.md"]);
});

test("generated-doc evaluator passes on current renders", () => {
  assert.deepEqual(evaluateGeneratedDocs(specs, current), []);
});

test("generated-doc evaluator fails on a missing doc", () => {
  const broken = new Map(current);
  broken.delete(specs[0].path);
  const failures = evaluateGeneratedDocs(specs, broken);
  assert.equal(failures.length, 1);
  assert.match(failures[0], /missing generated doc/);
});

test("generated-doc evaluator fails on a stale doc", () => {
  const broken = new Map(current);
  broken.set(specs[1].path, `${current.get(specs[1].path)}\nhand edit\n`);
  const failures = evaluateGeneratedDocs(specs, broken);
  assert.equal(failures.length, 1);
  assert.match(failures[0], /stale generated doc/);
});

test("README link evaluator passes when every local link resolves", () => {
  const failures = evaluateReadmeLinks("[docs](docs/reference/product-truth.md)", () => true);
  assert.deepEqual(failures, []);
});

test("README link evaluator fails on a deliberately broken link", () => {
  const failures = evaluateReadmeLinks(
    "[good](docs/reference/product-truth.md) [bad](docs/no-such-file.md)",
    (target) => target !== "docs/no-such-file.md",
  );
  assert.equal(failures.length, 1);
  assert.match(failures[0], /broken README link: docs\/no-such-file\.md/);
});

test("live source reports the current registry and Windows as sole tier-1 target", () => {
  // The current native registry includes Ledger, Push, Adapt and Cortex additions.
  assert.equal(truth.toolCount, 23);
  assert.deepEqual(platforms.tier1, ["Windows"]);
  assert.deepEqual(platforms.bestEffort, []);
});

let failed = 0;
for (const [name, fn] of checks) {
  try {
    fn();
    console.log(`ok - ${name}`);
  } catch (error) {
    failed += 1;
    console.error(`FAIL - ${name}: ${error.message}`);
  }
}
if (failed) {
  console.error(`check-docs self-test: ${failed}/${checks.length} failed`);
  process.exit(1);
}
console.log(`check-docs self-test: ${checks.length}/${checks.length} passed`);
