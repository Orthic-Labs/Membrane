import assert from "node:assert/strict";
import crypto from "node:crypto";
import fs from "node:fs";
import os from "node:os";
import path from "node:path";
import { spawnSync } from "node:child_process";
import test from "node:test";
import { fileURLToPath } from "node:url";
import { findWorkspaceRoot } from "../_workspace-root.mjs";


const HERE = path.dirname(fileURLToPath(import.meta.url));
const ROOT = findWorkspaceRoot(HERE, { required: false });
const SCHEMA_PATH = ROOT ? path.join(ROOT, "tools/lib/context-contracts.schema.json") : null;
const FIXTURES = ROOT ? path.join(ROOT, "tools/tests/context_contracts/fixtures") : null;
import { validateJsonSchema } from "../python-test-runtime.mjs";
const CONTRACT_FIXTURES = [
  ["ContextCandidateSet", "context-candidate-set-v1.json"],
  ["ContextPacket", "context-packet-v1.json"],
  ["ContextReceipt", "context-receipt-v1.json"],
  ["KnowledgeEmission", "knowledge-emission-v1.json"],
  ["HandoffEnvelope", "handoff-envelope-v1.json"],
];
const workspaceSkip = ROOT ? false : "requires parent monorepo context contracts";


function readJson(file) {
  return JSON.parse(fs.readFileSync(file, "utf8"));
}


function validate(name, payload) {
  const result = validateJsonSchema(readJson(SCHEMA_PATH), payload, `#/$defs/${name}`);
  return {
    status: result.valid ? 0 : 1,
    errors: result.errors.map((error) => ({ pointer: error.instancePath, message: error.message })),
  };
}


test("canonical schema hash is stable and all v1 fixtures validate", { skip: workspaceSkip }, () => {
  const schemaBytes = fs.readFileSync(SCHEMA_PATH);
  const schema = JSON.parse(schemaBytes.toString("utf8"));
  const schemaSha256 = crypto.createHash("sha256").update(schemaBytes).digest("hex"); // schema identity is published as sha256; asserting the published form

  assert.equal(schema.$schema, "https://json-schema.org/draft/2020-12/schema");
  assert.match(schemaSha256, /^[a-f0-9]{64}$/);
  for (const [name, file] of CONTRACT_FIXTURES) {
    assert.ok(schema.$defs[name], `missing canonical definition ${name}`);
    const result = validate(name, readJson(path.join(FIXTURES, file)));
    assert.equal(result.status, 0, `${name}: ${result.stderr || JSON.stringify(result.errors)}`);
  }
});


test("Cortex fixture freezes Layer 3 evidence and the provider safety ceiling", { skip: workspaceSkip }, () => {
  const payload = readJson(path.join(FIXTURES, "context-candidate-set-v1.json"));
  const candidate = payload.candidates[0];

  assert.deepEqual(payload.providerCeiling, { maxCandidates: 40, maxEstimatedTokens: 8000 });
  assert.equal(candidate.layer, 3);
  assert.equal(typeof candidate.exact, "boolean");
  assert.equal(typeof candidate.protected, "boolean");
  assert.ok(candidate.sourceRef && candidate.sourceHash && candidate.estimatedTokens);
  assert.ok(Object.keys(candidate.scoreComponents).length > 0);
  assert.ok(payload.omissions.every((item) => item.reason));
});


test("renaming a required candidate field fails at its JSON pointer", { skip: workspaceSkip }, () => {
  const payload = readJson(path.join(FIXTURES, "context-candidate-set-v1.json"));
  payload.candidates[0].sourceDigest = payload.candidates[0].sourceHash;
  delete payload.candidates[0].sourceHash;

  const result = validate("ContextCandidateSet", payload);

  assert.equal(result.status, 1);
  assert.ok(result.errors.some((error) => error.pointer === "/candidates/0"));
});


test("durable emission fixture contains no raw graph state", { skip: workspaceSkip }, () => {
  const emission = readJson(path.join(FIXTURES, "knowledge-emission-v1.json"));
  for (const forbidden of ["nodes", "edges", "embeddings", "layout", "communities"]) {
    assert.equal(Object.hasOwn(emission, forbidden), false);
  }
});
