import assert from "node:assert/strict";
import { readFileSync } from "node:fs";
import { join, resolve } from "node:path";
import test from "node:test";
import {
  REC_01,
  REC_02,
  REC_03,
  evaluateCanonReconciliation,
  evaluateCrosswalk,
  evaluateLegacyEvidence,
} from "./rec-windows.mjs";

const workspaceRoot = resolve(new URL("../../../", import.meta.url).pathname.replace(/^\/([A-Za-z]:)/, "$1"));
const reviewRoot = "D:/Claude/review/windows-r5";
const json = (file) => JSON.parse(readFileSync(file, "utf8"));

test("REC exports run.mjs-compatible source outcomes", async () => {
  for (const [id, fn] of [["REC-01", REC_01], ["REC-02", REC_02], ["REC-03", REC_03]]) {
    const result = await fn({ workspaceRoot });
    assert.equal(result.status, "passed", `${id}: ${result.reason}`);
    assert.equal(result.evidenceKind, "source");
  }
});

test("REC-01 negative controls reject stale generated docs and current Node Blueprint rows", () => {
  const clean = evaluateCanonReconciliation({
    canonTexts: Object.fromEntries(Array.from({ length: 7 }, (_, i) => [`${i}.md`, ""])),
    generatedChecks: [{ ok: true }],
  });
  assert.equal(clean.ok, true);
  assert.equal(evaluateCanonReconciliation({
    canonTexts: clean.canonTexts,
    generatedChecks: [{ ok: false }],
  }).ok, false);
  assert.equal(evaluateCanonReconciliation({
    canonTexts: clean.canonTexts,
    generatedChecks: [{ ok: true }],
    nodeRows: [{ file: "blueprint.md", line: 1 }],
  }).ok, false);
});

test("REC-02 negative control rejects a void row that is counted as qualified", () => {
  const registry = json(join(reviewRoot, "windows-acceptance.json"));
  const clean = evaluateLegacyEvidence(registry);
  assert.equal(clean.ok, true);
  const mutated = structuredClone(registry);
  mutated.cases.find((row) => row.legacyEvidenceVoid).canonicalQualificationRow = "| BPT-Q001 | BPT-001 | acceptance | PASS | evidence |";
  assert.equal(evaluateLegacyEvidence(mutated).ok, false);
  const unvoided = structuredClone(registry);
  unvoided.cases.find((row) => row.legacyEvidenceVoid).legacyEvidenceVoid = false;
  assert.equal(evaluateLegacyEvidence(unvoided).ok, false);
});

test("REC-03 negative controls reject denominator drift and an unmapped requirement", () => {
  const membrane = json(join(reviewRoot, "windows-acceptance.json"));
  const coderight = json(join(reviewRoot, "coderight-acceptance.json"));
  const blueprint = json(join(reviewRoot, "blueprint-membrane-acceptance.json"));
  const amendment = json(join(reviewRoot, "windows-amendment-acceptance.json"));
  const crosswalk = json(join(reviewRoot, "requirement-crosswalk.json"));
  const manifest = json(join(reviewRoot, "supplemental-inputs", "corrected-input-manifest.json"));
  const coderightAmendment = coderight;
  const base = { membrane, coderight, blueprint, amendment, coderightAmendment, crosswalk, manifest };
  assert.equal(evaluateCrosswalk(base).ok, true);
  const denominatorDrift = structuredClone(membrane);
  denominatorDrift.cases.pop();
  assert.equal(evaluateCrosswalk({ ...base, membrane: denominatorDrift }).ok, false);
  const unmapped = structuredClone(crosswalk);
  unmapped.rows[0].caseIds = [];
  assert.equal(evaluateCrosswalk({ ...base, crosswalk: unmapped }).ok, false);
});
