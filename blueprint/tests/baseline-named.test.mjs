import assert from "node:assert/strict";
import test from "node:test";

import { baselineFingerprintSet, changedSlice, captureNamedBaseline, listNamedBaselines, getNamedBaseline, clearNamedBaselines } from "../src/lib/rules/baseline.mjs";

// Helper to make findings with deterministic fingerprints
function makeFinding(path, fingerprint) {
  return { fingerprint, ruleId: "BP001", path, name: "x", specifier: "./m.js" };
}

test("backward compat: baselineFingerprintSet and changedSlice still work", () => {
  const baseline = { findings: [{ fingerprint: "f1" }, { fingerprint: "f2" }] };
  assert.deepEqual([...baselineFingerprintSet(baseline)], ["f1", "f2"]);
  const findings = [{ fingerprint: "f2" }, { fingerprint: "f3" }];
  const slice = changedSlice({ findings, baseline });
  assert.equal(slice.total, 2);
  assert.equal(slice.newCount, 1);
  assert.equal(slice.findings[0].fingerprint, "f3");
});

test("capture/list against NAMED generations store {name,generationId,findingsFingerprints}", () => {
  clearNamedBaselines();
  const f1 = makeFinding("src/a.ts", "fp-a1");
  const f2 = makeFinding("src/b.ts", "fp-b1");
  const captured = captureNamedBaseline("release-1", { generationId: "gen-1", findings: [f1, f2] });
  assert.equal(captured.name, "release-1");
  assert.equal(captured.generationId, "gen-1");
  assert.deepEqual(captured.findingsFingerprints.sort(), ["fp-a1", "fp-b1"].sort());
  assert.equal(captured.findingCount, 2);

  const listed = listNamedBaselines();
  assert.equal(listed.length, 1);
  assert.equal(listed[0].name, "release-1");
  assert.equal(listed[0].generationId, "gen-1");
  assert.deepEqual(listed[0].findingsFingerprints.sort(), ["fp-a1", "fp-b1"].sort());

  const fetched = getNamedBaseline("release-1");
  assert.equal(fetched.generationId, "gen-1");
  assert.ok(fetched.fingerprintSet.has("fp-a1"));
});
