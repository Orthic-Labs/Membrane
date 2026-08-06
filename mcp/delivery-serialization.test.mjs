import assert from "node:assert/strict";
import test from "node:test";
import {
  DELIVERY_METRICS_CONVENTIONS,
  DELIVERY_METRICS_SCHEMA,
  canonicalDeliveryMetrics,
} from "./delivery-serialization.mjs";

// MBR-012 qualification fixture: multi-byte UTF-8 (é = 2 bytes, ☕ = 3 bytes).
// The expected values are ALSO pinned in the Rust mirror test
// (crypt-store/src/context_telemetry.rs) so the two implementations are proven
// to agree byte-for-byte.
const FIXTURE = "Membrane café ☕";
const EXPECTED = {
  bytes: 18,
  chars: 15,
  tokens: 5,
  sha256: "sha256:8444cb5ce2a629fbeae804c4ec724128c835fd6885e37c98fb17bca2b5a3456e",
};

test("MBR-012: canonical metrics agree byte-for-byte on the qualification fixture", () => {
  const metrics = canonicalDeliveryMetrics(FIXTURE);
  assert.deepEqual(metrics, EXPECTED);
});

test("MBR-012: chars counts code points, not UTF-16 code units", () => {
  // "é" is one code point but 2 UTF-8 bytes; "☕" is one code point, 3 bytes.
  // String.length would overcount surrogate pairs; the canonical char count must not.
  const metrics = canonicalDeliveryMetrics(FIXTURE);
  assert.equal(metrics.chars, 15);
  assert.equal(FIXTURE.length >= metrics.chars, true, "UTF-16 length must not undercount");
  assert.equal(metrics.bytes, 18);
  assert.equal(metrics.bytes > metrics.chars, true, "multi-byte content has more bytes than chars");
});

test("MBR-012: tokens derive from bytes so they can never disagree", () => {
  const metrics = canonicalDeliveryMetrics(FIXTURE);
  assert.equal(metrics.tokens, Math.ceil(metrics.bytes / 4));
});

test("MBR-012: empty content has zero metrics and the well-known empty sha256", () => {
  assert.deepEqual(canonicalDeliveryMetrics(""), {
    bytes: 0,
    chars: 0,
    tokens: 0,
    sha256: "sha256:e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855",
  });
});

test("MBR-012: schema and conventions are stable and non-string input is empty", () => {
  assert.equal(DELIVERY_METRICS_SCHEMA, "orthic.delivery-metrics.v1");
  assert.equal(DELIVERY_METRICS_CONVENTIONS.encoding, "utf8");
  assert.equal(DELIVERY_METRICS_CONVENTIONS.tokens, "ceil(bytes/4)");
  assert.equal(canonicalDeliveryMetrics(undefined).bytes, 0);
  assert.equal(canonicalDeliveryMetrics(null).sha256, "sha256:e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855");
});
