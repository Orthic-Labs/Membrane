import test from "node:test";
import assert from "node:assert/strict";
import { readFileSync } from "node:fs";
import { validate } from "../../engine/crates/membrane-protocol/bindings/protocol.mjs";

const read = (path) => JSON.parse(readFileSync(new URL(`../../${path}`, import.meta.url), "utf8"));
const snapshotOperation = read("schemas/registry/operations/hub-snapshot.v1.golden.json");
const capabilitiesOperation = read("schemas/registry/operations/hub-capabilities.v1.golden.json");
const fixture = { snapshot: snapshotOperation.result.data, capabilities: capabilitiesOperation.result.data };
const snapshotSchema = read("schemas/hub-snapshot.v1.json");
const capabilitySchema = read("schemas/hub-capabilities.v1.json");
const resources = ["deliveries", "providers", "repositories", "adapters", "devices", "memory", "sentinel", "alerts"];

test("Hub facade is versioned, read-only, and never infers liveness", () => {
  assert.equal(fixture.capabilities.schemaVersion, 1);
  assert.equal(fixture.capabilities.readOnly, true);
  assert.deepEqual(fixture.capabilities.resources, resources);
  assert.deepEqual(fixture.capabilities.operations, ["hub.capabilities", "hub.snapshot"]);
  assert.deepEqual(snapshotSchema.required.slice(2), resources);
  assert.deepEqual(capabilitySchema.properties.resources.items.enum, resources);
  assert.deepEqual(validate(snapshotSchema, fixture.snapshot), []);
  assert.deepEqual(validate(capabilitySchema, fixture.capabilities), []);
  assert.deepEqual(snapshotOperation.result.data, fixture.snapshot);
  assert.deepEqual(capabilitiesOperation.result.data, fixture.capabilities);
  assert.equal(snapshotOperation.result.kind, "success");
  assert.equal(capabilitiesOperation.result.kind, "success");
  for (const field of ["installationId", "serviceId", "releaseGeneration", "dataRootDigest", "operations"])
    assert.ok(capabilitySchema.required.includes(field));
  for (const field of ["resolver", "source", "evidence", "observedAtUnixMs", "cacheAgeMs"])
    assert.ok(snapshotSchema.$defs.section.required.includes(field));
  for (const resource of resources) {
    const section = fixture.snapshot[resource];
    assert.ok(["available", "degraded", "unavailable"].includes(section.state));
    assert.ok(section.reason.length > 0);
    assert.ok(Array.isArray(section.items));
    assert.ok("resolver" in section && "source" in section && "evidence" in section);
  }
  assert.equal(fixture.snapshot.providers.items[0].status, "unknown");
  assert.equal(fixture.snapshot.adapters.state, "unavailable");
  assert.deepEqual(fixture.snapshot.adapters.items, []);
  const rejected = structuredClone(fixture.snapshot);
  rejected.providers.reason = "";
  assert.ok(validate(snapshotSchema, rejected).length > 0);
  delete rejected.alerts;
  assert.ok(validate(snapshotSchema, rejected).length > 0, "incomplete dispatch data is rejected");
});
