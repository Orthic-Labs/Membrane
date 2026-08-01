import assert from "node:assert/strict";
import test from "node:test";
import { calibrationHarness, fixedFusion } from "./retrieval-contracts.mjs";

const cases = [{ expected_ids: ["anchor"], candidates: [{ id: "anchor", provider_id: "live", rank: 1, scope_ok: true, fresh: true, utility: 4 }] }];

test("R2 calibration is held-out, deterministic, and fallback-safe", () => {
  const result = calibrationHarness({ training: cases, heldout: cases, candidatePolicy: fixedFusion, baselinePolicy: fixedFusion });
  assert.equal(result.schema, "orthic.retrieval-calibration.v1");
  assert.equal(result.deployable, false);
  assert.equal(result.fallback, "membrane-fusion-fixed-v1");
  assert.equal(result.candidate.scope_regressions, 0);
  assert.equal(result.diagnostics.ranking_mutation, "disabled_until_heldout_validation");
});

test("R2 calibration counts scope/freshness regressions", () => {
  const result = calibrationHarness({ heldout: [{ expected_ids: [], candidates: [{ id: "bad", provider_id: "x", scope_ok: false, fresh: false }] }], candidatePolicy: (candidates) => ({ candidates }) });
  assert.equal(result.candidate.scope_regressions, 1);
  assert.equal(result.candidate.freshness_regressions, 1);
  assert.equal(result.deployable, false);
});
