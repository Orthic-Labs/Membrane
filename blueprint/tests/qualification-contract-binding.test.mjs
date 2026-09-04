// Qualification contract-binding regression. The provider-qualification
// harness validates candidate output against the pinned packaged
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
  makeBlueprintStaticProvider,
  qualifyProvider,
} from "../evals/run-qualification.mjs";

const HERE = dirname(fileURLToPath(import.meta.url));
const ROOT = join(HERE, "..");
const SCHEMA = join(ROOT, "schemas", "context-candidate-set.v1.schema.json");
// Refresh the stale pre-consolidation pin, not the schema: a291e7dd (2026-08-20)
// is the current schema at the end of consolidation, including indexedAt. The root,
// registry and Rust projections independently bind this unchanged source.
const PINNED_SHA256 = "4d6bf47e42ba43d1b1501b443dd69c01468a6d59ff1b599349d4fb00891d06e1";
const REPOS = join(ROOT, "evals", "fixture-repos");
const TASKS = join(ROOT, "evals", "graph-tasks.jsonl");

test("packaged candidate contract matches the post-consolidation digest", () => {
  const bytes = readFileSync(SCHEMA);
  assert.equal(createHash("sha256").update(bytes).digest("hex"), PINNED_SHA256);
  const schema = JSON.parse(bytes.toString("utf8"));
  assert.equal(schema.$schema, "https://json-schema.org/draft/2020-12/schema");
  assert.equal(schema.type, "object");
  assert.ok(schema.required.includes("candidates"));
  assert.ok(schema.required.includes("indexedAt"));
  assert.equal(schema.$defs.candidate.additionalProperties, false);
  assert.ok(schema.$defs.candidate && schema.$defs.omission && schema.$defs.freshness);
});

test("blueprint-static passes contract and portability gates against the pinned contract", async () => {
  const tasks = loadTasks(TASKS).filter((task) => task.qualificationClass === "mandatory_structural");
  const provider = makeBlueprintStaticProvider({ schemaPath: SCHEMA });
  try {
    const report = await qualifyProvider(provider, tasks, REPOS);
    assert.equal(report.status, "passed");
    assert.equal(report.gates.contract, true);
    assert.equal(report.gates.portability, true);
  } finally {
    await provider.close?.();
  }
});
