import assert from "node:assert/strict";
import { readFileSync } from "node:fs";
import test from "node:test";
import { classifyProtectedEvidence, protectedBytes, PROTECTED_EVIDENCE_KINDS } from "./protection-classifier.mjs";

const fixtures = JSON.parse(readFileSync(new URL("./fixtures/protected-evidence-adversarial.json", import.meta.url), "utf8"));

test("MBR-603 adversarial evidence spans remain byte-identical", () => {
  for (const fixture of fixtures) {
    const spans = classifyProtectedEvidence(fixture);
    assert.equal(spans.length > 0, fixture.protected !== false, fixture.name);
    assert.deepEqual(protectedBytes(fixture.text, spans), Buffer.from(fixture.expected), fixture.name);
    assert.ok(spans.every(({ start, end }) => Number.isSafeInteger(start) && start >= 0 && end > start && end <= Buffer.byteLength(fixture.text)));
  }
});

test("explicit evidence kinds and flags protect complete blocks", () => {
  const text = "first α\nsecond\r\n";
  for (const kind of PROTECTED_EVIDENCE_KINDS) assert.deepEqual(protectedBytes(text, classifyProtectedEvidence({ text, kind })), Buffer.from(text));
  assert.deepEqual(protectedBytes(text, classifyProtectedEvidence({ text, protected: true })), Buffer.from(text));
  assert.deepEqual(classifyProtectedEvidence({ text: "ordinary progress output\n" }), []);
  assert.throws(() => classifyProtectedEvidence({ text: null }), /must be a string/);
});
