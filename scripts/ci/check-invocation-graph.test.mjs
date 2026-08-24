#!/usr/bin/env node
// Tests for scripts/ci/check-invocation-graph.mjs and parts of
// scripts/ci/build-invocation-graph.mjs (migration spec N0).
import assert from "node:assert/strict";
import { mkdtempSync, mkdirSync, rmSync, writeFileSync } from "node:fs";
import { tmpdir } from "node:os";
import { join } from "node:path";
import { test } from "node:test";

import {
  TRAVERSABLE_BOUNDARIES,
  extractRepoPathLiterals,
  isExecutableCandidate,
  resolvePythonModule,
} from "./build-invocation-graph.mjs";
import { SEAL_REL, deriveReachability, rowAgreement, validateInvocationGraph } from "./check-invocation-graph.mjs";

const node = (id, kind = "tracked-executable", production_reachable = false) => ({
  id,
  kind,
  runtime: id.endsWith(".py") ? "python" : id.endsWith(".rs") ? "rust" : "node",
  production_reachable,
});

test("traversable boundary set contains exactly the traversed boundaries", () => {
  for (const b of ["in-process", "import", "module", "process", "loopback-http", "external-typed-protocol", "stdio", "packaged-projection"]) {
    assert.ok(TRAVERSABLE_BOUNDARIES.has(b), `missing ${b}`);
  }
  assert.ok(!TRAVERSABLE_BOUNDARIES.has("path-reference"));
  assert.ok(!TRAVERSABLE_BOUNDARIES.has("data"));
});

test("deriveReachability BFS follows traversable edges only", () => {
  const graph = {
    productionEntrypoints: [{ id: "mcp/server.mjs" }],
    nodes: [],
    edges: [
      { id: "e1", from: "mcp/server.mjs", to: "mcp/lib.cjs", boundary: "import", origin: "scanned", evidence: ["t"] },
      // weak evidence must NOT traverse:
      { id: "e2", from: "mcp/lib.cjs", to: "scripts/orphan.py", boundary: "path-reference", origin: "scanned", evidence: ["t"] },
      // process edge does traverse:
      { id: "e3", from: "mcp/server.mjs", to: "engine/crates/membrane/src/main.rs", boundary: "process", origin: "curated", evidence: ["spec"] },
    ],
  };
  const reach = deriveReachability(graph);
  assert.ok(reach.has("mcp/lib.cjs"));
  assert.equal(reach.get("mcp/lib.cjs").seed, "mcp/server.mjs");
  assert.equal(reach.get("mcp/lib.cjs").hops, 1);
  assert.ok(reach.has("engine/crates/membrane/src/main.rs"));
  assert.ok(!reach.has("scripts/orphan.py"), "path-reference edges must not confer reachability");
});

test("rowAgreement: agrees when flag matches graph-derived reachability", () => {
  const reachable = new Set(["a.py", "b.js"]);
  assert.deepEqual(rowAgreement({ id: "r1", runtime: "python", files: ["a.py"], production_reachable: true }, reachable), { agrees: true, expected: true });
  assert.deepEqual(rowAgreement({ id: "r2", runtime: "python", files: ["zzz.py"], production_reachable: false }, reachable), { agrees: true, expected: false });
  const disagree = rowAgreement({ id: "r3", runtime: "python", files: ["zzz.py"], production_reachable: true }, reachable);
  assert.equal(disagree.agrees, false);
  assert.equal(disagree.expected, false);
});

test("rowAgreement: external typed-service rows are exempted from file execution checks", () => {
  assert.equal(rowAgreement({ id: "bp", runtime: "external", files: [], production_reachable: true }, new Set()).agrees, true);
});

function makeTmpRoot() {
  return mkdtempSync(join(tmpdir(), "inv-graph-test-"));
}

test("validateInvocationGraph flags ghost nodes, unreachable drift, and legacy-ledger regressions", () => {
  const root = makeTmpRoot();
  try {
    mkdirSync(join(root, "mcp"), { recursive: true });
    writeFileSync(join(root, "mcp", "server.mjs"), "console.log(1);\n");
    writeFileSync(join(root, "mcp", "lib.cjs"), "module.exports = {};\n");

    const graph = {
      artifact: "membrane.invocation-graph",
      schemaVersion: 2,
      baselineCommit: "deadbeef",
      productionEntrypoints: [{ id: "mcp/server.mjs" }],
      nodes: [
        node("mcp/server.mjs", "tracked-executable", true),
        node("mcp/lib.cjs", "tracked-executable", true),
        node("deleted/ghost.py", "tracked-executable", false), // git no longer tracks it
        node("mcp/orphan.py", "tracked-executable", true), // claims reachability but no edge reaches it
      ],
      edges: [
        { id: "e1", from: "mcp/server.mjs", to: "mcp/lib.cjs", boundary: "import", origin: "scanned", evidence: ["x"] },
        { id: "e2", from: "mcp/server.mjs", to: "missing-node.py", boundary: "process", origin: "curated", evidence: ["y"] }, // dangling endpoint
      ],
      derivedProductionFiles: ["mcp/server.mjs"],
      unresolvedReferences: [{ reference: "../outside/thing.py", from: "mcp/lib.cjs", reason: "escapes repo" }],
    };

    const manifest = {
      totals: { rows: 1 },
      rows: [
        // disagrees with graph-derived reachability (no file reached):
        // references a file with no reachable graph node at all:
        { id: "row-mcp-never", runtime: "node", files: ["mcp/never-reached.cjs"], production_reachable: true },
      ],
    };

    const reconciliation = {
      status: "superseded",
      mappings: [
        { legacyId: "art-1", canonicalTarget: "mcp/server.mjs" },
        { legacyId: "art-1", canonicalTarget: "duplicate" }, // duplicate mapping
      ],
      gatesConsumingLegacyLedger: ["some/gate.mjs"],
    };

    const trackedFiles = ["mcp/server.mjs", "mcp/lib.cjs"];
    const { errors } = validateInvocationGraph({ root, graph, manifest, reconciliation, trackedFiles });
    const codes = errors.map((e) => e.code);
    assert.ok(codes.includes("STALE_GRAPH_GHOST_NODE"));
    assert.ok(codes.includes("REACHABILITY_NOT_REPRODUCIBLE"));
    assert.ok(codes.includes("EDGE_UNKNOWN_ENDPOINT"));
    assert.ok(codes.includes("MANIFEST_ROW_DISAGREES_WITH_GRAPH"));
    assert.ok(codes.includes("RECONCILIATION_DUPLICATE_MAPPING"));
    assert.ok(codes.includes("GATE_CONSUMES_LEGACY_LEDGER"));

    // Clean variant passes.
    const cleanGraph = {
      ...graph,
      nodes: [node("mcp/server.mjs", "tracked-executable", true), node("mcp/lib.cjs", "tracked-executable", true)],
      edges: [graph.edges[0]],
    };
    const cleanManifest = { rows: [{ id: "row-ok", runtime: "node", files: ["mcp/lib.cjs"], production_reachable: true }] };
    const cleanReconciliation = { status: "superseded", mappings: [{ legacyId: "art-1", canonicalTarget: "mcp/server.mjs" }], gatesConsumingLegacyLedger: [] };
    const clean = validateInvocationGraph({
      root,
      graph: cleanGraph,
      manifest: cleanManifest,
      reconciliation: cleanReconciliation,
      trackedFiles,
    });
    assert.deepEqual(clean.errors, [], JSON.stringify(clean.errors));
  } finally {
    rmSync(root, { recursive: true, force: true });
  }
});

test("native-only seal honesty guard blocks a premature seal", () => {
  const root = makeTmpRoot();
  try {
    const graph = {
      artifact: "membrane.invocation-graph",
      schemaVersion: 2,
      productionEntrypoints: [],
      nodes: [node("still-python/tool.py", "tracked-executable", true)],
      edges: [],
      unresolvedReferences: [{ reference: "x", from: "y", reason: "z" }],
    };
    const manifest = {
      rows: [{ id: "py-row", runtime: "python", files: ["still-python/tool.py"], production_reachable: true }],
    };
    mkdirSync(join(root, "migration", "native-rust"), { recursive: true });
    writeFileSync(join(root, SEAL_REL), "{}");
    const { errors } = validateInvocationGraph({ root, graph, manifest, reconciliation: null, trackedFiles: [] });
    assert.ok(errors.some((e) => e.code === "LEGACY_LEDGER_NOT_SUPERSEDED"));
    assert.ok(errors.some((e) => e.code === "NATIVE_ONLY_SEAL_PREMATURE"));
  } finally {
    rmSync(root, { recursive: true, force: true });
  }
});

test("isExecutableCandidate classifies extensions and shebangs", () => {
  const root = makeTmpRoot();
  try {
    mkdirSync(join(root, "tools"), { recursive: true });
    writeFileSync(join(root, "tools", "run.sh"), "#!/usr/bin/env bash\necho hi\n");
    writeFileSync(join(root, "tools", "notes.txt"), "not executable\n");
    assert.equal(isExecutableCandidate("tools/run.sh", root), true);
    assert.equal(isExecutableCandidate("tools/notes.txt", root), false);
    assert.equal(isExecutableCandidate("src/mod.py", null), true);
    assert.equal(isExecutableCandidate("src/mod.rs", null), true);
    assert.equal(isExecutableCandidate("src/app.mjs", null), true);
    assert.equal(isExecutableCandidate("README.md", null), false);
  } finally {
    rmSync(root, { recursive: true, force: true });
  }
});

test("resolvePythonModule maps irregular packages and relative imports correctly", () => {
  assert.deepEqual(
    resolvePythonModule("adapt.insights.detectors", "adapt/cli.py").candidates,
    ["adapt/src/adapt/insights/detectors.py", "adapt/src/adapt/insights/detectors/__init__.py"],
  );
  assert.deepEqual(
    resolvePythonModule("federation.gateway", "x/y.py").candidates,
    ["engine/federation/gateway.py", "engine/federation/gateway/__init__.py"],
  );
  assert.deepEqual(resolvePythonModule(".sibling", "pkg/sub/entry.py").candidates, [
    "pkg/sub/sibling.py",
    "pkg/sub/sibling/__init__.py",
  ]);
  assert.deepEqual(resolvePythonModule("..other", "pkg/sub/entry.py").candidates, [
    "pkg/other.py",
    "pkg/other/__init__.py",
  ]);
  assert.deepEqual(resolvePythonModule("os", "x.py"), { candidates: [] });
});

test("extractRepoPathLiterals distinguishes launch-context strong references from weak ones", () => {
  const strong = extractRepoPathLiterals('spawn(process.execPath, ["tools/gen.mjs"]);');
  assert.ok(strong.some((l) => l.token === "tools/gen.mjs" && l.launch === true), JSON.stringify(strong));

  const weak = extractRepoPathLiterals('const fallback = "tools/gen.mjs";');
  assert.ok(weak.some((l) => l.token === "tools/gen.mjs" && l.launch === false), JSON.stringify(weak));
});
