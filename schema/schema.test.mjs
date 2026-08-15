import test from "node:test";
import assert from "node:assert/strict";
import {
  validateManifestV2, validateSnapshotV2, validateLifecycleFrame,
  evaluateHubCompatRange, checkFixtures, checkBundle, computeBundleDigest,
} from "./validate.mjs";

test("hubCompatRange mirrors the documented grammar & fail-closed semantics", () => {
  for (const r of [">=0.1", ">=0.1.0", ">=0", "*", "", ">=0.1.0 <1.0.0", ">=0.1.0, <1.0.0", "==0.1.11"]) assert.ok(evaluateHubCompatRange(r, "0.1.11"), r);
  for (const r of [">=1.0.0", "<0.1.0", "~0.1.0", "^1.0", ">=0.1.0.0", ">=0.x.0", "banana"]) assert.equal(evaluateHubCompatRange(r, "0.1.11"), false, r);
  assert.ok(evaluateHubCompatRange("=0.1", "0.1.0"));
  assert.equal(evaluateHubCompatRange("=0.1", "0.1.11"), false);
});

test("fixtures: valid accept, adversarial reject", () => {
  const errors = checkFixtures();
  assert.deepEqual(errors, [], errors.join("\n"));
});

test("bundle digest is reproducible & metadata complete", () => {
  const errors = checkBundle();
  assert.deepEqual(errors, [], errors.join("\n"));
});

test("snapshot bounds reject non-object items, nested values, arbitrary maps, & total-byte cap", () => {
  assert.equal(validateSnapshotV2({ schemaVersion: 2, productId: "cortex", observedAtUnixMs: 1, sections: { x: { state: "available", reason: "ok", items: ["bare-string"] } } }), "snapshot_item_not_object");
  assert.equal(validateSnapshotV2({ schemaVersion: 2, productId: "cortex", observedAtUnixMs: 1, sections: { x: { state: "available", reason: "ok", items: [{ label: "d1", count: { a: 1 } }] } } }), "snapshot_item_value_not_primitive");
  assert.equal(validateSnapshotV2({ schemaVersion: 2, productId: "cortex", observedAtUnixMs: 1, sections: { x: { state: "available", reason: "ok", items: [{ label: "d1", count: [1, 2] }] } } }), "snapshot_item_value_not_primitive");
  assert.equal(validateSnapshotV2({ schemaVersion: 2, productId: "cortex", observedAtUnixMs: 1, sections: { x: { state: "available", reason: "ok", items: [{ label: "d1", rogueKey: "v" }] } } }), "snapshot_item_field_unknown");
  assert.equal(validateSnapshotV2({ schemaVersion: 2, productId: "cortex", observedAtUnixMs: 1, sections: { x: { state: "available", reason: "ok", items: [{ kind: "delivery" }] } } }), "snapshot_item_string_too_long");
  // Total payload cap matches the live runtime (65536).
  const big = { schemaVersion: 2, productId: "cortex", observedAtUnixMs: 1, sections: { x: { state: "available", reason: "ok", items: [{ label: "d1", evidence: "E".repeat(70000) }] } } };
  const stored = big.sections.x.items[0].evidence;
  big.sections.x.items[0].evidence = "E".repeat(70000);
  assert.equal(validateSnapshotV2(big), "snapshot_too_large");
  big.sections.x.items[0].evidence = stored;
});

test("snapshot items reject wrong scalar types, invalid enum, negative count, bad observedAtUnixMs, non-boolean stale (per-field exactness)", () => {
  const base = { schemaVersion: 2, productId: "cortex", observedAtUnixMs: 1, sections: { x: { state: "available", reason: "ok", items: [1] } } };
  const it = (item) => ({ ...base, sections: { x: { state: "available", reason: "ok", items: [item] } } });
  // missing/null/empty/wrong-type required label
  assert.equal(validateSnapshotV2(it({})), "snapshot_item_string_too_long");
  assert.equal(validateSnapshotV2(it({ label: null })), "snapshot_item_string_too_long");
  assert.equal(validateSnapshotV2(it({ label: "" })), "snapshot_item_string_too_long");
  assert.equal(validateSnapshotV2(it({ label: 3 })), "snapshot_item_string_too_long");
  // count wrong scalar type / negative (schema minimum 0)
  assert.equal(validateSnapshotV2(it({ label: "d1", count: "3" })), "snapshot_item_value_not_primitive");
  assert.equal(validateSnapshotV2(it({ label: "d1", count: true })), "snapshot_item_value_not_primitive");
  assert.equal(validateSnapshotV2(it({ label: "d1", count: null })), "snapshot_item_value_not_primitive");
  assert.equal(validateSnapshotV2(it({ label: "d1", count: -1 })), "snapshot_item_value_not_primitive");
  // severity invalid enum / wrong type
  assert.equal(validateSnapshotV2(it({ label: "d1", severity: "bogus" })), "snapshot_item_value_not_primitive");
  assert.equal(validateSnapshotV2(it({ label: "d1", severity: 2 })), "snapshot_item_value_not_primitive");
  // observedAtUnixMs fractional / negative / wrong type (schema: integer, minimum 0)
  assert.equal(validateSnapshotV2(it({ label: "d1", observedAtUnixMs: 1.5 })), "snapshot_item_value_not_primitive");
  assert.equal(validateSnapshotV2(it({ label: "d1", observedAtUnixMs: -1 })), "snapshot_item_value_not_primitive");
  assert.equal(validateSnapshotV2(it({ label: "d1", observedAtUnixMs: "now" })), "snapshot_item_value_not_primitive");
  // stale non-boolean / null
  assert.equal(validateSnapshotV2(it({ label: "d1", stale: "yes" })), "snapshot_item_value_not_primitive");
  assert.equal(validateSnapshotV2(it({ label: "d1", stale: 0 })), "snapshot_item_value_not_primitive");
  assert.equal(validateSnapshotV2(it({ label: "d1", stale: null })), "snapshot_item_value_not_primitive");
  // kind / evidence wrong scalar type
  assert.equal(validateSnapshotV2(it({ label: "d1", kind: 7 })), "snapshot_item_string_too_long");
  assert.equal(validateSnapshotV2(it({ label: "d1", evidence: false })), "snapshot_item_string_too_long");
  // a fully valid closed item still accepts
  assert.equal(validateSnapshotV2(it({ label: "d1", kind: "delivery", count: 3, severity: "warning", evidence: "hub#1", resolver: "retry", observedAtUnixMs: 999, stale: false })), null);
  // count:0 and a valid fractional-but-... no: schema count is number>=0; 0.5 is allowed (number, not negative)
  assert.equal(validateSnapshotV2(it({ label: "d1", count: 0 })), null);
});

test("manifest rejects inline v1 secret/endpoint as v2", () => {
  const base = { schemaVersion: 2, productId: "cortex", displayName: "Cortex", productVersion: "1.0.0", hubCompatRange: ">=0.1.0", installRoot: "/x", serviceStart: ["/x/bin"], serviceStop: [], icon: "/x/icon.png", artifactDigest: "sha256:aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa" };
  assert.equal(validateManifestV2({ ...base, statusEndpoint: { host: "127.0.0.1", port: 8080, authHeader: "X", authToken: "t" } }), "manifest_schema_invalid");
  assert.equal(validateManifestV2({ ...base, artifactDigest: "sha256:deadbeef" }), "manifest_artifact_digest_invalid");
  assert.equal(validateManifestV2({ ...base, hubCompatRange: ">=99.0.0" }), "manifest_hub_range_incompatible");
});

test("lifecycle rejects future version & unknown frame/command", () => {
  const hello = { kind: "hello", lifecycleVersion: 1, installationId: "i", productId: "cortex", instanceId: "i:1", fence: 1, artifactDigest: "sha256:aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa", declaredDataRoot: "/x", secret: "0123456789abcdef0123456789abcdef0123456789abcdef0123456789abcdef" };
  assert.equal(validateLifecycleFrame({ ...hello, lifecycleVersion: 2 }), "lifecycle_version_unsupported");
  assert.equal(validateLifecycleFrame({ kind: "command", command: "reboot", fence: 1 }), "unknown_lifecycle_command");
  assert.equal(validateLifecycleFrame({ kind: "register", state: "ready", fence: 1, endpoint: { host: "10.0.0.1", port: 9 }, capability: "c" }), "endpoint_not_loopback");
});

test("computeBundleDigest is stable across calls", () => {
  assert.equal(computeBundleDigest(), computeBundleDigest());
});