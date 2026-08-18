// MBR-912: proves the fail-closed release-channel compatibility policy is
// machine-checkable, not merely documented. This is the one test file in
// this change that actually runs (`node --test
// tests/compat/release-channel-compatibility.test.mjs`); the Rust
// counterpart (engine/crates/membrane-protocol/src/compatibility_policy.rs)
// carries the same cases as #[test]s reasoned through by type, per this
// task's hard rule against running cargo.
import test from "node:test";
import assert from "node:assert/strict";
import { evaluateCompatibility, evaluateReleaseChannel } from "../../dist/release/channels/compatibility-policy.mjs";

const descriptor = (overrides = {}) => ({
  schemaVersion: 1,
  channel: "stable",
  release: "1.1.0",
  schemaCompatibility: "compatible",
  ...overrides,
});

test("unknown channel is refused", () => {
  const result = evaluateCompatibility("1.0.0", descriptor({ channel: "canary" }));
  assert.equal(result.admitted, false);
  assert.ok(result.violations.some((v) => v.code === "release_channel_unknown"));
});

test("unsupported schema version is refused", () => {
  const result = evaluateCompatibility("1.0.0", descriptor({ schemaVersion: 2 }));
  assert.equal(result.admitted, false);
  assert.ok(result.violations.some((v) => v.code === "release_channel_schema_version_unsupported"));
});

test("unrecognized schemaCompatibility string is refused", () => {
  const result = evaluateCompatibility("1.0.0", descriptor({ schemaCompatibility: "maybe" }));
  assert.equal(result.admitted, false);
  assert.ok(result.violations.some((v) => v.code === "release_channel_schema_compatibility_unknown"));
});

test("explicit schemaCompatibility=unknown is refused, never treated as compatible", () => {
  const result = evaluateCompatibility("1.0.0", descriptor({ schemaCompatibility: "unknown" }));
  assert.equal(result.admitted, false);
  assert.ok(result.violations.some((v) => v.code === "release_channel_schema_compatibility_unknown"));
});

test("incompatible schema is refused", () => {
  const result = evaluateCompatibility("1.0.0", descriptor({ schemaCompatibility: "incompatible" }));
  assert.equal(result.admitted, false);
  assert.deepEqual(result.violations.map((v) => v.code), ["release_channel_schema_incompatible"]);
});

test("downgrade is refused", () => {
  const result = evaluateCompatibility("1.0.0", descriptor({ release: "0.9.0" }));
  assert.equal(result.admitted, false);
  const downgrade = result.violations.find((v) => v.code === "release_channel_downgrade_rejected");
  assert.ok(downgrade);
  assert.deepEqual(downgrade.detail, { current: "1.0.0", candidate: "0.9.0" });
});

test("equal version is refused, not treated as an upgrade", () => {
  const result = evaluateCompatibility("1.0.0", descriptor({ release: "1.0.0" }));
  assert.equal(result.admitted, false);
  assert.ok(result.violations.some((v) => v.code === "release_channel_downgrade_rejected"));
});

test("unparseable candidate version is refused", () => {
  const result = evaluateCompatibility("1.0.0", descriptor({ release: "latest" }));
  assert.equal(result.admitted, false);
  assert.ok(result.violations.some((v) => v.code === "release_channel_version_unparseable"));
});

test("unparseable current (locally installed) version is refused", () => {
  const result = evaluateCompatibility("not-a-version", descriptor());
  assert.equal(result.admitted, false);
  assert.ok(result.violations.some((v) => v.code === "release_channel_version_unparseable"));
});

test("every independent violation is collected, not just the first", () => {
  const result = evaluateCompatibility(
    "1.0.0",
    descriptor({ channel: "canary", schemaVersion: 2, schemaCompatibility: "incompatible", release: "0.9.0" }),
  );
  assert.equal(result.admitted, false);
  assert.equal(result.violations.length, 4);
});

test("a compatible upgrade on a known channel is admitted", () => {
  const result = evaluateCompatibility("1.0.0", descriptor());
  assert.equal(result.admitted, true);
  assert.equal(result.channel, "stable");
  assert.equal(result.release, "1.1.0");
  assert.equal(result.migrationRequired, false);
});

test("a migration_required upgrade is admitted and flagged, not refused", () => {
  const result = evaluateCompatibility(
    "1.9.0",
    descriptor({ channel: "beta", release: "2.0.0", schemaCompatibility: "migration_required" }),
  );
  assert.equal(result.admitted, true);
  assert.equal(result.migrationRequired, true);
});

test("evaluateReleaseChannel adapts a schema-valid object through the same path", () => {
  const schemaValid = {
    schemaVersion: 1,
    channel: "nightly",
    release: "1.2.0-nightly.3",
    support: { state: "supported", startsAt: "2026-01-01", endsAt: null },
    schemaCompatibility: "compatible",
    migration: "no migration required",
    rollback: "restore prior release",
    signedUpdateEvidence: null,
  };
  const result = evaluateReleaseChannel("1.1.0", schemaValid);
  assert.equal(result.admitted, true);
  assert.equal(result.channel, "nightly");
  assert.equal(result.release, "1.2.0-nightly.3");
});

test("a null/missing signedUpdateEvidence never implies an admitted update decision by itself", () => {
  // evaluateReleaseChannel/evaluateCompatibility never read signedUpdateEvidence --
  // admission is about channel/schema/version only. This test pins that the
  // policy's admitted:true carries no claim about update availability, so a
  // caller cannot mistake "admitted" for "update evidence present."
  const schemaValid = {
    schemaVersion: 1,
    channel: "stable",
    release: "1.1.0",
    support: { state: "supported", startsAt: "2026-01-01", endsAt: null },
    schemaCompatibility: "compatible",
    migration: "n/a",
    rollback: "n/a",
    signedUpdateEvidence: null,
  };
  const result = evaluateReleaseChannel("1.0.0", schemaValid);
  assert.equal(result.admitted, true);
  assert.equal("signedUpdateEvidence" in result, false);
});
