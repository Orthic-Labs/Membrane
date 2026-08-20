// D44: semantic sidecar + hybrid reranker — candidates only, disabled by
// default, every rank explainable by components.

import assert from "node:assert/strict";
import test from "node:test";

import { semanticCandidateProvider, semanticEnabled } from "../src/providers/ranking/semantic.mjs";
import { hybridRank, SCORE_COMPONENTS } from "../src/providers/ranking/hybrid.mjs";

test("semantic provider is candidate-only and disabled by default", async () => {
  assert.equal(semanticEnabled(), false);
  const probe = await semanticCandidateProvider.probe();
  assert.equal(probe.state, "disabled");
  const result = await semanticCandidateProvider.search({ query: "x", generationId: "g1", limit: 10 });
  assert.equal(result.candidates.length, 0);
  assert.ok(result.omissions.some((o) => o.reason === "semantic_provider_disabled"));
  assert.equal(semanticCandidateProvider.authority, "candidate-only");
});

test("semantic provider is opt-in via BLUEPRINT_SEMANTIC=1", () => {
  const previous = process.env.BLUEPRINT_SEMANTIC;
  process.env.BLUEPRINT_SEMANTIC = "1";
  try {
    assert.equal(semanticEnabled(), true);
  } finally {
    if (previous === undefined) delete process.env.BLUEPRINT_SEMANTIC;
    else process.env.BLUEPRINT_SEMANTIC = previous;
  }
});

test("hybrid rank exposes every component separately", () => {
  const ranked = hybridRank({
    candidates: [
      { id: "a", exactAnchor: 1, lexicalScore: 0.5, evidenceConfidence: 0.9, structuralProximity: 0.8 },
      { id: "b", exactAnchor: 0, lexicalScore: 0.9, evidenceConfidence: 0.6 },
    ],
    semanticCandidates: [{ id: "b", score: 0.7 }],
  });
  assert.equal(ranked.length, 2);
  for (const entry of ranked) {
    for (const component of SCORE_COMPONENTS) {
      assert.ok(component in entry.components, `missing component ${component}`);
    }
  }
  assert.equal(ranked[0].provider, "hybrid");
  // b gets a semantic boost.
  assert.ok(ranked.find((r) => r.id === "b").components.semanticScore > 0);
});

test("hybrid ranking is deterministic with stable tie-break", () => {
  const a = hybridRank({ candidates: [{ id: "x", lexicalScore: 0.5 }, { id: "y", lexicalScore: 0.5 }] });
  const b = hybridRank({ candidates: [{ id: "x", lexicalScore: 0.5 }, { id: "y", lexicalScore: 0.5 }] });
  assert.deepEqual(a.map((r) => r.id), b.map((r) => r.id));
});
