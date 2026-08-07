import test from "node:test";
import assert from "node:assert/strict";
import { readFileSync } from "node:fs";
import { join } from "node:path";

const schema = JSON.parse(readFileSync(join(process.cwd(), "schemas/compression-receipt.v1.schema.json"), "utf8"));
test("compression receipt schema is closed and requires recovery metadata", () => {
  assert.equal(schema.properties.schemaVersion.const, 1);
  for (const key of ["originalHash", "transform", "transformVersion", "keptSpans", "protectedSpans", "dropped", "resolver", "risk", "tokenDelta"]) assert.ok(schema.required.includes(key));
  assert.equal(schema.additionalProperties, false);
  assert.equal(schema.properties.originalHash.pattern, "^sha256:[0-9a-f]{64}$");
  assert.equal(schema.properties.dropped.items.$ref, "#/$defs/dropped");
  assert.equal(schema.$defs.span.additionalProperties, false);
  assert.equal(schema.$defs.span.properties.start.minimum, 0);
  assert.equal(schema.$defs.span.properties.end.minimum, 0);
  assert.equal(schema.$defs.span.properties.start.maximum, 4294967295);
  assert.equal(schema.$defs.span.properties.end.maximum, 4294967295);
  for (const key of ["transform", "transformVersion", "resolver", "risk"]) assert.equal(schema.properties[key].minLength, 1);
  assert.equal(schema.$defs.dropped.properties.reason.minLength, 1);
  assert.equal(schema.$defs.dropped.properties.span.$ref, "#/$defs/span");
  const hash = `sha256:${"a".repeat(64)}`;
  assert.match(hash, new RegExp(schema.properties.originalHash.pattern));
  assert.doesNotMatch(`sha256:${"A".repeat(64)}`, new RegExp(schema.properties.originalHash.pattern));
});
