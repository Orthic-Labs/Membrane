import assert from "node:assert/strict";
import fs from "node:fs";
import os from "node:os";
import path from "node:path";
import test from "node:test";

import {
  EDGE_CONFIDENCE_TIERS,
  EDGE_CONFIDENCE_TIER_ORDER,
  tierConfidence,
  isTierAtLeast,
  filterEdgesByMinTier,
} from "../src/graph/confidence-tiers.mjs";
import { PRECISION_TIERS, PRECISION_TIER_ORDER } from "../src/graph/precision-tiers.mjs";
import { probeScip, augmentGenerationWithScip } from "../src/graph/scip-provider.mjs";
import {
  buildGraphGeneration,
  graphCapabilities,
  graphPrecisionProbe,
} from "../src/graph/static-provider.mjs";
import { buildTreeSitterGraph, PROVIDER as TREESITTER_PROVIDER } from "../src/graph/treesitter-provider.mjs";

function withFiles(files, run) {
  const repo = fs.mkdtempSync(path.join(os.tmpdir(), "cortex-tiers-"));
  try {
    for (const [relativePath, contents] of Object.entries(files)) {
      const absolutePath = path.join(repo, relativePath);
      fs.mkdirSync(path.dirname(absolutePath), { recursive: true });
      fs.writeFileSync(absolutePath, contents);
    }
    run(repo);
  } finally {
    fs.rmSync(repo, { recursive: true, force: true });
  }
}

// ---------------------------------------------------------------------------
// B3 — confidence-tier vocabulary
// ---------------------------------------------------------------------------

test("EDGE_CONFIDENCE_TIERS is ordered most -> least certain with a pure numeric mapping", () => {
  assert.deepEqual(EDGE_CONFIDENCE_TIER_ORDER, [
    "EXACT_RESOLUTION",
    "SAME_FILE_LEXICAL",
    "CROSS_FILE_HEURISTIC",
    "UNRESOLVED",
  ]);
  assert.equal(tierConfidence(EDGE_CONFIDENCE_TIERS.EXACT_RESOLUTION), 1);
  assert.ok(tierConfidence(EDGE_CONFIDENCE_TIERS.SAME_FILE_LEXICAL) < 1);
  assert.ok(tierConfidence(EDGE_CONFIDENCE_TIERS.CROSS_FILE_HEURISTIC) < tierConfidence(EDGE_CONFIDENCE_TIERS.SAME_FILE_LEXICAL));
  assert.equal(tierConfidence(EDGE_CONFIDENCE_TIERS.UNRESOLVED), 0);
  assert.throws(() => tierConfidence("MADE_UP_TIER"));
});

test("isTierAtLeast compares tier certainty, not string equality", () => {
  assert.ok(isTierAtLeast(EDGE_CONFIDENCE_TIERS.EXACT_RESOLUTION, EDGE_CONFIDENCE_TIERS.UNRESOLVED));
  assert.ok(!isTierAtLeast(EDGE_CONFIDENCE_TIERS.UNRESOLVED, EDGE_CONFIDENCE_TIERS.EXACT_RESOLUTION));
  assert.ok(isTierAtLeast(EDGE_CONFIDENCE_TIERS.SAME_FILE_LEXICAL, EDGE_CONFIDENCE_TIERS.CROSS_FILE_HEURISTIC));
});

test("cortex-static: resolved IMPORTS is EXACT_RESOLUTION, same-file CALLS is SAME_FILE_LEXICAL, cross-file import-linked CALLS is CROSS_FILE_HEURISTIC", () => {
  withFiles({
    "src/service.ts": [
      "export function localHelper() { return 1; }",
      "export function run() { return localHelper(); }",
    ].join("\n"),
    "src/consumer.ts": [
      "import { run } from './service';",
      "export function invoke() { return run(); }",
    ].join("\n"),
  }, (repo) => {
    const generation = buildGraphGeneration(repo);

    const importEdge = generation.edges.find((e) => e.kind === "IMPORTS" && e.source === "file:src/consumer.ts");
    assert.equal(importEdge.confidenceTier, "EXACT_RESOLUTION");
    assert.equal(importEdge.confidence, 1);
    assert.equal(importEdge.resolved, true);

    const sameFileCall = generation.edges.find((e) => e.kind === "CALLS" && e.source.includes("::run") && e.target.includes("localHelper"));
    assert.equal(sameFileCall.confidenceTier, "SAME_FILE_LEXICAL");

    const crossFileCall = generation.edges.find((e) => e.kind === "CALLS" && e.source.includes("::invoke") && e.target.includes("::run"));
    assert.equal(crossFileCall.confidenceTier, "CROSS_FILE_HEURISTIC");
  });
});

test("cortex-static: unresolved relative import is tagged UNRESOLVED, target null, never dropped", () => {
  withFiles({
    "src/broken.ts": "import { missing } from './does-not-exist';\nexport function use() { return missing(); }\n",
  }, (repo) => {
    const generation = buildGraphGeneration(repo);
    const unresolved = generation.edges.find((e) => e.kind === "IMPORTS" && e.source === "file:src/broken.ts");
    assert.ok(unresolved, "unresolved import edge must be emitted, not silently dropped");
    assert.equal(unresolved.target, null);
    assert.equal(unresolved.resolved, false);
    assert.equal(unresolved.confidenceTier, "UNRESOLVED");
    assert.equal(unresolved.confidence, 0);
    assert.equal(unresolved.specifier, "./does-not-exist");
  });
});

test("cortex-static: an ambiguous call (name exists in 2+ unrelated files, no import link) is tagged UNRESOLVED and kept, never guessed", () => {
  withFiles({
    "src/one.ts": "export function sharedName() { return 1; }\n",
    "src/two.ts": "export function sharedName() { return 2; }\n",
    "src/caller.ts": "export function callIt() { return sharedName(); }\n",
  }, (repo) => {
    const generation = buildGraphGeneration(repo);
    const callsFromCaller = generation.edges.filter((e) => e.kind === "CALLS" && e.source.includes("::callIt"));
    // No resolved edge — neither candidate is in the same file or imported.
    assert.ok(!callsFromCaller.some((e) => e.target !== null));
    const unresolved = callsFromCaller.find((e) => e.target === null);
    assert.ok(unresolved, "ambiguous call must be tagged, not dropped");
    assert.equal(unresolved.confidenceTier, "UNRESOLVED");
    assert.equal(unresolved.specifier, "sharedName");
  });
});

test("cortex-static: every edge (resolved or not) carries a tier from the shared vocabulary", () => {
  withFiles({
    "src/a.ts": "import './b';\nexport function fromA() { return fromB(); }\n",
    "src/b.ts": "export function fromB() { return 1; }\n",
  }, (repo) => {
    const generation = buildGraphGeneration(repo);
    assert.ok(generation.edges.length > 0);
    for (const e of generation.edges) {
      assert.ok(EDGE_CONFIDENCE_TIER_ORDER.includes(e.confidenceTier), `edge ${e.id} missing a known confidenceTier`);
      assert.equal(e.confidence, tierConfidence(e.confidenceTier));
    }
  });
});

test("filterEdgesByMinTier keeps only edges at least as certain as the floor", () => {
  withFiles({
    "src/a.ts": "import './b';\nexport function fromA() { return fromB(); }\n",
    "src/b.ts": "export function fromB() { return 1; }\n",
  }, (repo) => {
    const generation = buildGraphGeneration(repo);
    const exactOnly = filterEdgesByMinTier(generation.edges, EDGE_CONFIDENCE_TIERS.EXACT_RESOLUTION);
    assert.ok(exactOnly.every((e) => e.confidenceTier === "EXACT_RESOLUTION"));
    assert.ok(exactOnly.length < generation.edges.length || generation.edges.every((e) => e.confidenceTier === "EXACT_RESOLUTION"));
  });
});

test("cortex-treesitter: unresolved import and ambiguous call are tagged UNRESOLVED, never dropped", async () => {
  const files = [
    { path: "src/broken.ts", text: "import { missing } from './nope';\nexport function use() { return missing(); }\n" },
    { path: "src/one.ts", text: "export function sharedName() { return 1; }\n" },
    { path: "src/two.ts", text: "export function sharedName() { return 2; }\n" },
    { path: "src/caller.ts", text: "export function callIt() { return sharedName(); }\n" },
  ];
  const graph = await buildTreeSitterGraph(files);

  const unresolvedImport = graph.edges.find((e) => e.kind === "IMPORTS" && e.source === "file:src/broken.ts");
  assert.ok(unresolvedImport);
  assert.equal(unresolvedImport.target, null);
  assert.equal(unresolvedImport.confidenceTier, "UNRESOLVED");
  assert.equal(unresolvedImport.confidence, 0);

  const ambiguousCall = graph.edges.find((e) => e.kind === "CALLS" && e.source.includes("::callIt") && e.target === null);
  assert.ok(ambiguousCall, "ambiguous cross-file call must be tagged, not dropped");
  assert.equal(ambiguousCall.confidenceTier, "UNRESOLVED");
  assert.equal(ambiguousCall.specifier, "sharedName");

  for (const e of graph.edges) {
    assert.ok(EDGE_CONFIDENCE_TIER_ORDER.includes(e.confidenceTier), `edge ${e.id} missing a known confidenceTier`);
  }
});

// ---------------------------------------------------------------------------
// B4 — precision tiers + SCIP honest degrade
// ---------------------------------------------------------------------------

test("PRECISION_TIERS is ordered COMPILER > AST > LEXICAL", () => {
  assert.deepEqual(PRECISION_TIER_ORDER, ["COMPILER", "AST", "LEXICAL"]);
});

test("each provider declares its own precision-tier ceiling", () => {
  const staticCaps = graphCapabilities();
  assert.equal(staticCaps.precisionTier, PRECISION_TIERS.LEXICAL);
  assert.equal(TREESITTER_PROVIDER.precisionTier, PRECISION_TIERS.AST);
});

test("probeScip degrades honestly to AST when no index is present (never fabricates one)", () => {
  withFiles({ "src/a.ts": "export const x = 1;\n" }, (repo) => {
    const probe = probeScip(repo);
    assert.equal(probe.state, "unavailable");
    assert.equal(probe.precisionTier, "COMPILER");
    assert.equal(probe.degradesTo, "AST");
    assert.ok(/no SCIP index found/.test(probe.reason));
  });
});

test("probeScip degrades honestly when index.scip.json exists but is malformed", () => {
  withFiles({ "index.scip.json": "{ not valid json" }, (repo) => {
    const probe = probeScip(repo);
    assert.equal(probe.state, "unavailable");
    assert.ok(/could not be parsed as JSON/.test(probe.reason));
  });
});

test("probeScip degrades honestly when the JSON shape has no documents array", () => {
  withFiles({ "index.scip.json": JSON.stringify({ notDocuments: [] }) }, (repo) => {
    const probe = probeScip(repo);
    assert.equal(probe.state, "unavailable");
    assert.ok(/not a recognized portable-SCIP-JSON shape/.test(probe.reason));
  });
});

test("probeScip reports ok and reads a well-formed portable SCIP index without inventing data", () => {
  withFiles({
    "src/a.ts": "export function foo() { return 1; }\n",
    "index.scip.json": JSON.stringify({
      documents: [{ relativePath: "src/a.ts", occurrences: [{ symbol: "foo", roles: ["reference"], range: [0, 0, 0, 3] }] }],
    }),
  }, (repo) => {
    const probe = probeScip(repo);
    assert.equal(probe.state, "ok");
    assert.equal(probe.documentCount, 1);
  });
});

test("augmentGenerationWithScip: absence degrades the generation's augmentation report, does not touch edges", async () => {
  withFiles({ "src/a.ts": "export function foo() { return 1; }\n" }, async (repo) => {
    const generation = buildGraphGeneration(repo);
    const edgesBefore = generation.edges.length;
    const result = await augmentGenerationWithScip(generation, repo);
    assert.equal(result.state, "unavailable");
    assert.equal(generation.augmentation.scip.state, "unavailable");
    assert.equal(generation.edges.length, edgesBefore, "no edges may be invented when no SCIP index exists");
  });
});

test("augmentGenerationWithScip: presence adds only real REFERENCES edges tagged EXACT_RESOLUTION, joined against existing nodes", async () => {
  withFiles({
    "src/a.ts": "export function foo() { return 1; }\n",
  }, async (repo) => {
    const generation = buildGraphGeneration(repo);
    const symbolNode = generation.nodes.find((n) => n.kind === "symbol" && n.qualifiedName === "foo");
    assert.ok(symbolNode, "fixture must actually produce a foo symbol node to join against");
    fs.writeFileSync(path.join(repo, "index.scip.json"), JSON.stringify({
      documents: [{ relativePath: "src/a.ts", occurrences: [{ symbol: "foo", roles: ["reference"], range: [0, 0, 0, 3] }] }],
    }));
    const edgesBefore = generation.edges.length;
    const result = await augmentGenerationWithScip(generation, repo);
    assert.equal(result.state, "ok");
    assert.equal(result.edgesAdded, 1);
    assert.equal(generation.edges.length, edgesBefore + 1);
    const added = generation.edges.at(-1);
    assert.equal(added.kind, "REFERENCES");
    assert.equal(added.confidenceTier, "EXACT_RESOLUTION");
    assert.equal(added.target, symbolNode.id);
  });
});

test("graphPrecisionProbe reports LEXICAL and AST as ok, COMPILER as unavailable without an index", () => {
  withFiles({ "src/a.ts": "export const x = 1;\n" }, (repo) => {
    const probe = graphPrecisionProbe(repo);
    const byTier = Object.fromEntries(probe.tiers.map((t) => [t.tier, t]));
    assert.equal(byTier.LEXICAL.state, "ok");
    assert.equal(byTier.AST.state, "ok");
    assert.equal(byTier.COMPILER.state, "unavailable");
    assert.equal(probe.highestAvailable, "AST");
  });
});

test("graphCapabilities exposes the precision tier and the edge confidence tier vocabulary on the probe surface", () => {
  const caps = graphCapabilities();
  assert.equal(caps.precisionTier, "LEXICAL");
  assert.deepEqual(caps.precisionTiers.order, PRECISION_TIER_ORDER);
  assert.deepEqual(caps.edgeConfidenceTiers.order, EDGE_CONFIDENCE_TIER_ORDER);
  assert.ok(caps.edgeConfidenceTiers.descriptions.UNRESOLVED);
});
