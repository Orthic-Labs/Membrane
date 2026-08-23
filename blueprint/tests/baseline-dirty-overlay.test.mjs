import assert from "node:assert/strict";
import test from "node:test";

import { baselineFingerprintSet, changedSlice, captureNamedBaseline, listNamedBaselines, getNamedBaseline, clearNamedBaselines, dirtyOverlayDelta } from "../src/lib/rules/baseline.mjs";

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

test("dirtyOverlayDelta recomputes only overlay without rescanning untouched files", () => {
  clearNamedBaselines();
  const baselineFindings = [
    makeFinding("src/a.ts", "fp-a1"),
    makeFinding("src/b.ts", "fp-b1"),
    makeFinding("src/c.ts", "fp-c1"),
  ];
  captureNamedBaseline("base", { generationId: "gen-10", findings: baselineFindings });

  // Current: a unchanged, b resolved (removed), d added (new), c unchanged but not dirty
  const currentFindings = [
    makeFinding("src/a.ts", "fp-a1"), // unchanged, in dirty overlay
    makeFinding("src/c.ts", "fp-c1"), // unchanged, NOT dirty — should not be rescanned but still classified
    makeFinding("src/d.ts", "fp-d1"), // added, in dirty overlay
  ];
  // dirtyPaths = only a and d and b touched
  const delta = dirtyOverlayDelta("base", ["src/a.ts", "src/b.ts", "src/d.ts"], { currentFindings, currentGenerationId: "gen-10" });

  assert.equal(delta.baselineName, "base");
  assert.equal(delta.baselineGenerationId, "gen-10");
  // added: fp-d1 (dirty)
  assert.equal(delta.added.length, 1);
  assert.equal(delta.added[0].fingerprint, "fp-d1");
  // resolved: fp-b1 (was in baseline, not in current, and dirty)
  assert.equal(delta.resolved.length, 1);
  assert.equal(delta.resolved[0].fingerprint, "fp-b1");
  // unchanged: fp-a1 and fp-c1 (both persist)
  assert.equal(delta.unchanged.length, 2);
  assert.ok(delta.unchanged.some((f) => f.fingerprint === "fp-a1"));
  assert.ok(delta.unchanged.some((f) => f.fingerprint === "fp-c1"));
  // overlaySize reflects dirtyPaths length
  assert.equal(delta.stats.overlaySize, 3);
  assert.equal(delta.omissions.length, 0);
});

test("dirtyOverlayDelta overlay filtering: untouched added/resolved outside overlay are not reported", () => {
  clearNamedBaselines();
  captureNamedBaseline("base2", { generationId: "gen-2", findings: [makeFinding("src/a.ts", "fp-a")] });
  // Current adds a finding in src/z.ts which is NOT in dirtyPaths
  const currentFindings = [makeFinding("src/a.ts", "fp-a"), makeFinding("src/z.ts", "fp-z")];
  const deltaDirty = dirtyOverlayDelta("base2", ["src/a.ts"], { currentFindings, currentGenerationId: "gen-2" });
  // fp-z is added but not dirty, so dirty-filtered delta reports 0 added
  assert.equal(deltaDirty.added.length, 0);
  // Without dirty filter, it would report
  const deltaAll = dirtyOverlayDelta("base2", [], { currentFindings, currentGenerationId: "gen-2" });
  assert.equal(deltaAll.added.length, 1);
});

test("dirtyOverlayDelta typed omissions when baseline missing", () => {
  clearNamedBaselines();
  const result = dirtyOverlayDelta("nonexistent", ["src/a.ts"], { currentFindings: [], currentGenerationId: "gen-1" });
  assert.equal(result.omissions.length, 1);
  assert.equal(result.omissions[0].code, "baseline_missing");
  assert.match(result.omissions[0].detail, /nonexistent/);
  assert.equal(result.added.length, 0);
  assert.equal(result.resolved.length, 0);
});

test("dirtyOverlayDelta typed omissions when baseline stale", () => {
  clearNamedBaselines();
  captureNamedBaseline("stale-base", { generationId: "gen-old", findings: [makeFinding("src/a.ts", "fp-a")] });
  const currentFindings = [makeFinding("src/a.ts", "fp-a")];
  const result = dirtyOverlayDelta("stale-base", ["src/a.ts"], { currentFindings, currentGenerationId: "gen-new" });
  assert.equal(result.omissions.length, 1);
  assert.equal(result.omissions[0].code, "baseline_stale");
  assert.equal(result.omissions[0].reason, "stale");
  assert.equal(result.omissions[0].expectedGenerationId, "gen-old");
  assert.equal(result.omissions[0].observedGenerationId, "gen-new");
  // still classifies
  assert.equal(result.unchanged.length, 1);
});

test("dirtyOverlayDelta overload: array as third arg treated as currentFindings", () => {
  clearNamedBaselines();
  captureNamedBaseline("overload", { generationId: "g1", findings: [makeFinding("src/a.ts", "fp-a")] });
  const delta = dirtyOverlayDelta("overload", ["src/a.ts"], [makeFinding("src/a.ts", "fp-a"), makeFinding("src/b.ts", "fp-b")]);
  assert.equal(delta.added.length, 1);
  assert.equal(delta.added[0].fingerprint, "fp-b");
});
