import test from "node:test";
import assert from "node:assert/strict";
import { readFileSync } from "node:fs";
import { validate } from "../../engine/crates/membrane-protocol/bindings/protocol.mjs";
const read = (name) => JSON.parse(readFileSync(new URL(name, import.meta.url), "utf8"));
const fixture = read("multidimensional-status.v1.golden.json");
const schema = read("../../schemas/multidimensional-status.v1.json");
const rank = { available: 0, degraded: 1, unavailable: 2 };
test("multidimensional status fixture matches its closed schema", () => {
  assert.deepEqual(validate(schema, fixture), []);
  assert.equal(schema.additionalProperties, false);
  for (const dimension of ["installation", "adapter", "delivery"])
    assert.equal(schema.$defs[dimension].additionalProperties, false);
});
test("aggregate parity prevents false healthy rendering", () => {
  const dimensions = [fixture.installation, fixture.adapter, fixture.delivery];
  const aggregate = dimensions.reduce((worst, item) => rank[item.state] > rank[worst] ? item.state : worst, "available");
  assert.equal(fixture.state, aggregate);
  assert.equal(fixture.installation.reason, "wrong_installation");
  assert.notEqual(fixture.state, "available");
  assert.equal(fixture.delivery.reason, "selected_without_delivery");
  assert.equal(fixture.delivery.alert, "selected_without_delivery");
});
test("schema rejects incomplete, unknown, and invalid status data", () => {
  const incomplete = structuredClone(fixture);
  delete incomplete.installation.reason;
  assert.ok(validate(schema, incomplete).length > 0);
  const unknown = structuredClone(fixture);
  unknown.delivery.raw = "untyped";
  assert.ok(validate(schema, unknown).length > 0);
  const invalid = structuredClone(fixture);
  invalid.adapter.status = "healthy";
  assert.ok(validate(schema, invalid).length > 0);
});
