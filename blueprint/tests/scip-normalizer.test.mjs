import assert from "node:assert/strict";
import { dirname, join } from "node:path";
import test from "node:test";
import { fileURLToPath } from "node:url";

import { augmentGenerationWithScip } from "../src/graph/scip-provider.mjs";
import { pythonScipProvider } from "../src/providers/compilers/python-scip.mjs";
import { normalizeScipIndex, normalizeScipRoles, readNormalizedScipIndex } from "../src/providers/compilers/scip-normalize.mjs";

const PYTHON_FIXTURE = join(dirname(fileURLToPath(import.meta.url)), "fixtures", "compiler-adapters", "python");
const INDEX = join(PYTHON_FIXTURE, "index.scip.json");

test("SCIP role normalization gives string and standard bitmask forms the same contract", () => {
  assert.deepEqual([...normalizeScipRoles(["definition", "read"])].sort(), ["definition", "read"]);
  assert.deepEqual([...normalizeScipRoles(1 | 4)].sort(), ["definition", "read"]);
  assert.deepEqual([...normalizeScipRoles(2 | 8)].sort(), ["reference", "write"]);
});

test("canonical SCIP normalizer builds exact cross-document definition identity", () => {
  const index = readNormalizedScipIndex(INDEX);
  const totalSymbol = [...index.definitionsBySymbol.keys()].find((symbol) => symbol.endsWith("Item#total()."));
  assert.ok(totalSymbol, "fixture exposes the exact total() SCIP symbol");
  const definition = index.definitionsBySymbol.get(totalSymbol);
  assert.ok(definition, "total() definition is indexed by exact SCIP symbol");
  assert.equal(definition.documentPath, "pkg/models.py");

  const crossDocumentReference = index.occurrences.find(
    (occurrence) => occurrence.documentPath === "pkg/service.py"
      && occurrence.symbol === totalSymbol
      && occurrence.roles.has("reference"),
  );
  assert.ok(crossDocumentReference);
  assert.equal(index.definitionsBySymbol.get(crossDocumentReference.symbol), definition);
});

test("normalizer preserves symbol information, relationships, external symbols, and position metadata", () => {
  const index = normalizeScipIndex({
    metadata: { version: "fixture", textDocumentEncoding: "UTF8" },
    documents: [{
      relativePath: "src/a.ts",
      occurrences: [{ symbol: "scip-typescript npm pkg 1.0 src/a Foo#", roles: 1, range: [0, 0, 0, 3] }],
      symbols: [{
        symbol: "scip-typescript npm pkg 1.0 src/a Foo#",
        displayName: "Foo",
        documentation: ["A documented type."],
        relationships: [{ symbol: "scip-typescript npm pkg 1.0 src/base Base#", isImplementation: true }],
      }],
    }],
    externalSymbols: [{
      symbol: "scip-typescript npm dep 2.0 index External#",
      displayName: "External",
      relationships: [{ symbol: "scip-typescript npm dep 2.0 index Parent#", isTypeDefinition: true }],
    }],
  });

  assert.equal(index.metadata.textDocumentEncoding, "UTF8");
  const foo = index.symbolInformationBySymbol.get("scip-typescript npm pkg 1.0 src/a Foo#");
  assert.equal(foo.displayName, "Foo");
  assert.deepEqual(foo.documentation, ["A documented type."]);
  assert.equal(foo.relationships[0].isImplementation, true);
  assert.equal(index.externalSymbols.length, 1);
  assert.equal(index.externalSymbols[0].relationships[0].isTypeDefinition, true);
});

test("generic and Python SCIP lanes share exact normalized symbol targets", async () => {
  const python = await pythonScipProvider.collect({ repoRoot: PYTHON_FIXTURE });
  const totalDefinition = python.nodes.find((node) => node.symbol?.endsWith("Item#total()."));
  assert.ok(totalDefinition);

  const paths = ["pkg/models.py", "pkg/service.py", "main.py"];
  const generation = {
    nodes: paths.map((path) => ({
      id: `file:${path}`,
      kind: "file",
      name: path,
      qualifiedName: path,
      path,
      evidence: [{ path, startLine: 1, endLine: 1, contentHash: `hash:${path}` }],
    })),
    edges: [],
    manifest: { counts: { nodes: paths.length, edges: 0 } },
  };

  const applied = await augmentGenerationWithScip(generation, PYTHON_FIXTURE);
  assert.equal(applied.state, "ok");
  assert.equal(applied.applied, true);
  const genericDefinition = generation.nodes.find((node) => node.symbol === totalDefinition.symbol);
  assert.ok(genericDefinition);
  assert.equal(genericDefinition.id, totalDefinition.id, "both consumers use the same exact SCIP symbol identity");

  const genericReference = generation.edges.find(
    (edge) => edge.kind === "REFERENCES"
      && edge.evidence?.[0]?.path === "pkg/service.py"
      && edge.evidence?.[0]?.symbol === totalDefinition.symbol,
  );
  assert.ok(genericReference);
  assert.equal(genericReference.target, totalDefinition.id);
  assert.equal(genericReference.confidenceTier, "EXACT_RESOLUTION");
});
