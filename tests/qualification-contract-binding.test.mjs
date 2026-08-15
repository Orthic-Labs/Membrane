// Qualification contract-binding regression. The provider-qualification
// harness validates candidate output against the pinned released
// membrane.context-candidate-set.v1 contract (exact version + digest) rather
// than an optional sibling-source schema, and the single-host portability gate
// passes for portable (no-native-dependency) providers. This pins the repair
// that replaced the obsolete `tools/lib/context-contracts.schema.json` default.

import assert from "node:assert/strict";
import { createHash } from "node:crypto";
import { readFileSync } from "node:fs";
import { dirname, join } from "node:path";
import { fileURLToPath } from "node:url";
import test from "node:test";

import {
  loadTasks,
  makeCortexStaticProvider,
  qualifyProvider,
} from "../evals/run-qualification.mjs";

const HERE = dirname(fileURLToPath(import.meta.url));
const ROOT = join(HERE, "..");
const SCHEMA = join(ROOT, "schemas", "context-candidate-set.v1.schema.json");
const PINNED_SHA256 = "c749f6b99133169d6c662b1852a7a831d411b01f6bd2a75867a69e301af16a5d";
const REPOS = join(ROOT, "evals", "fixture-repos");
const TASKS = join(ROOT, "evals", "graph-tasks.jsonl");

test("pinned candidate contract is an exact-digest released artifact", () => {
  const bytes = readFileSync(SCHEMA);
  assert.equal(createHash("sha256").update(bytes).digest("hex"), PINNED_SHA256);
  const schema = JSON.parse(bytes.toString("utf8"));
  assert.equal(schema.$schema, "https://json-schema.org/draft/2020-12/schema");
  assert.equal(schema.type, "object");
  assert.ok(schema.required.includes("candidates"));
  assert.ok(schema.$defs.candidate && schema.$defs.omission && schema.$defs.freshness);
});

test("cortex-static passes contract and portability gates against the pinned contract", async () => {
  const tasks = loadTasks(TASKS).filter((task) => task.qualificationClass === "mandatory_structural");
  const provider = makeCortexStaticProvider({ schemaPath: SCHEMA });
  try {
    const report = await qualifyProvider(provider, tasks, REPOS);
    assert.equal(report.status, "passed");
    assert.equal(report.gates.contract, true);
    assert.equal(report.gates.portability, true);
  } finally {
    await provider.close?.();
  }
});
