import assert from "node:assert/strict";
import { mkdirSync, mkdtempSync, readFileSync, rmSync, writeFileSync } from "node:fs";
import { tmpdir } from "node:os";
import { dirname, join } from "node:path";
import test from "node:test";
import { fileURLToPath } from "node:url";

import { assertSemanticConformance, verifySemanticConformance } from "../src/graph/conformance-verifier.mjs";
import { buildGraphGeneration } from "../src/graph/static-provider.mjs";

const FIXTURE_ROOT = join(dirname(fileURLToPath(import.meta.url)), "fixtures", "semantic-conformance");

function loadFixture(name) {
  return JSON.parse(readFileSync(join(FIXTURE_ROOT, `${name}.json`), "utf8"));
}

function buildFixture(fixture) {
  const root = mkdtempSync(join(tmpdir(), "blueprint-conformance-"));
  for (const [path, text] of Object.entries(fixture.files ?? {})) {
    const target = join(root, path);
    mkdirSync(dirname(target), { recursive: true });
    writeFileSync(target, text);
  }
  return { root, generation: buildGraphGeneration(root) };
}

for (const name of ["exact-import-call", "module-ambiguity"]) {
  test(`semantic conformance fixture passes: ${name}`, () => {
    const fixture = loadFixture(name);
    const { root, generation } = buildFixture(fixture);
    try {
      const report = assertSemanticConformance(generation, fixture);
      assert.equal(report.status, "passed");
      assert.equal(report.fixture, fixture.name);
      assert.equal(report.assertions.length, fixture.assertions.length);
      assert.ok(report.providers.some((provider) => provider.id === "blueprint-static"));
      assert.ok(report.assertions.every((assertion) => assertion.passed));
    } finally {
      rmSync(root, { recursive: true, force: true });
    }
  });
}

test("semantic conformance failure names the exact violated assertion", () => {
  const fixture = loadFixture("exact-import-call");
  const { root, generation } = buildFixture(fixture);
  try {
    const broken = {
      ...fixture,
      name: "intentional-failure",
      assertions: [{
        id: "missing-definition-contract",
        type: "node_exists",
        where: { path: "src/nope.ts", name: "notThere" },
      }],
    };
    const report = verifySemanticConformance(generation, broken);
    assert.equal(report.status, "failed");
    assert.match(report.failures[0], /missing-definition-contract/);
    assert.throws(
      () => assertSemanticConformance(generation, broken),
      (error) => error.code === "semantic_conformance_failed" && /missing-definition-contract/.test(error.message),
    );
  } finally {
    rmSync(root, { recursive: true, force: true });
  }
});
