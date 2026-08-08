import test from "node:test";
import assert from "node:assert/strict";
import { readFileSync } from "node:fs";
import { validate } from "../../engine/crates/membrane-protocol/bindings/protocol.mjs";
const read = (name) => JSON.parse(readFileSync(new URL(name, import.meta.url), "utf8"));
const fixture = read("provider-readiness.v1.golden.json");
const schema = read("../../schemas/provider-readiness.v1.schema.json");

test("provider readiness golden matches closed schema", () => {
  assert.deepEqual(validate(schema, fixture), []);
  for (const state of ["ready", "degraded", "unavailable", "unknown"])
    assert.deepEqual(validate(schema, {...fixture, state}), []);
});

test("provider readiness rejects missing identity, stale shape, and unknown fields", () => {
  const missing = structuredClone(fixture); delete missing.identity.serviceId;
  const malformed = structuredClone(fixture); malformed.observation.freshForMs = 0;
  const unknown = {...fixture, processAlive: true};
  assert.ok(validate(schema, missing).length);
  assert.ok(validate(schema, malformed).length);
  assert.ok(validate(schema, unknown).length);
});
