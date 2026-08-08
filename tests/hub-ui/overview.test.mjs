import test from "node:test";
import assert from "node:assert/strict";
import { readFile } from "node:fs/promises";
import { RESOURCES, sortedAlerts, normalizeSnapshot } from "../../apps/membrane-hub/src/overview.mjs";
test("overview keeps exact eight resources and read-only shell", async () => {
  const html = await readFile(new URL("../../apps/membrane-hub/index.html", import.meta.url), "utf8");
  assert.deepEqual(RESOURCES, ["deliveries","providers","repositories","adapters","devices","memory","sentinel","alerts"]);
  assert.match(html, /read-only Hub snapshot/); assert.match(html, /importmap/); assert.doesNotMatch(html, /set_startup|checkbox/);
});
test("error and stale envelopes remain explicit", () => {
  assert.equal(normalizeSnapshot({ result: { kind: "error", code: "x" } }).envelope.kind, "error");
  assert.equal(normalizeSnapshot({ payload: { schemaVersion: 1, cached: true } }).stale, true);
});
test("canonical result.data and cached wrapper preserve observed metadata", () => {
  const canonical = normalizeSnapshot({ result: { kind: "success", data: { schemaVersion: 1, observedAtUnixMs: 42, deliveries: { state: "unknown" } } } });
  assert.equal(canonical.data.observedAtUnixMs, 42);
  const cached = normalizeSnapshot({ schema_version: 1, observed_at_unix_ms: 7, payload: { schemaVersion: 1 } });
  assert.equal(cached.data.observedAtUnixMs, 7);
  assert.equal(normalizeSnapshot({ result: { kind: "success" } }).envelope.code, "snapshot_invalid");
});
test("alerts sort by stable severity then reason", () => {
  const sorted = sortedAlerts([{severity:"info",reason:"z"},{severity:"critical",reason:"b"},{severity:"critical",reason:"a"}]);
  assert.deepEqual(sorted.map(a=>a.reason), ["a","b","z"]);
});
