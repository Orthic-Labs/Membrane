import assert from "node:assert/strict";
import test from "node:test";

import { Bm25CodeIndex, buildBm25CodeIndex, tokenizeCodeIdentifiers } from "../src/graph/bm25-code-index.mjs";
import { searchAstStructure } from "../src/graph/ast-structural-search.mjs";
import { projectSymbolSignatures } from "../src/graph/signature-projection.mjs";
import { buildColdStartOrientation } from "../src/graph/orientation.mjs";

function ev(path, line = 1) { return [{ path, startLine: line, endLine: line, contentHash: `h:${path}` }]; }
function generation() {
  return {
    manifest: { generationId: "g-retrieval" },
    nodes: [
      { id: "file:src/order-service.ts", kind: "file", path: "src/order-service.ts", labels: ["File"], evidence: ev("src/order-service.ts") },
      { id: "symbol:OrderService", kind: "symbol", path: "src/order-service.ts", name: "OrderService", qualifiedName: "OrderService", labels: ["Class"], evidence: ev("src/order-service.ts", 1) },
      { id: "symbol:placeOrder", kind: "symbol", path: "src/order-service.ts", name: "placeOrder", qualifiedName: "OrderService.placeOrder", labels: ["Method"], declaringType: "OrderService", rawDeclaredType: "placeOrder(input: OrderInput): Promise<Order>", evidence: ev("src/order-service.ts", 4) },
      { id: "symbol:processPayment", kind: "symbol", path: "src/payment.ts", name: "processPayment", qualifiedName: "processPayment", labels: ["Function"], entryPoint: true, evidence: ev("src/payment.ts", 2) },
      { id: "domain:route", kind: "domain", path: null, name: "GET /orders", qualifiedName: "route:GET /orders", labels: ["HttpRoute"], method: "GET", routePath: "/orders", entryPoint: true, evidence: ev("src/routes.ts", 8) },
    ],
    edges: [
      { id: "call", kind: "CALLS", source: "symbol:processPayment", target: "symbol:placeOrder", confidenceTier: "EXACT_RESOLUTION", resolved: true, evidence: ev("src/payment.ts", 3) },
      { id: "route", kind: "ROUTES_TO", source: "domain:route", target: "symbol:placeOrder", confidenceTier: "EXACT_RESOLUTION", resolved: true, evidence: ev("src/routes.ts", 8) },
    ],
  };
}

const files = [
  { path: "src/order-service.ts" }, { path: "src/payment.ts" }, { path: "src/routes.ts" }, { path: "tests/order-service.test.ts" },
];

test("identifier tokenizer splits camelCase snake paths and punctuation", () => {
  assert.deepEqual(tokenizeCodeIdentifiers("OrderService.place_order src/payments-api.ts"), ["order", "service", "place", "order", "src", "payments", "api", "ts"]);
});

test("BM25 code index ranks exact names first and supports incremental replacement", () => {
  const index = buildBm25CodeIndex(generation());
  assert.equal(index.search("placeOrder", { limit: 3 })[0].id, "symbol:placeOrder");
  assert.ok(index.search("order service", { limit: 5 }).some((row) => row.id === "symbol:OrderService"));
  index.replaceDocument({ id: "new", name: "placeOrder", qualifiedName: "Other.placeOrder", path: "src/new.ts" });
  assert.deepEqual(index.search("placeOrder", { limit: 2 }).map((row) => row.id).sort(), ["new", "symbol:placeOrder"].sort());
  index.removeDocument("new");
  assert.equal(index.search("placeOrder", { limit: 1 })[0].id, "symbol:placeOrder");
});

test("AST structural search filters canonical structural facts instead of regexing source", () => {
  const result = searchAstStructure(generation(), { kind: "Method", declaringType: "OrderService", name: "place" });
  assert.deepEqual(result.nodes.map((node) => node.id), ["symbol:placeOrder"]);
  const withRelation = searchAstStructure(generation(), { kind: "Method", relation: "CALLS" });
  assert.ok(withRelation.edges.some((edge) => edge.id === "call"));
});

test("compact signatures preserve source anchors and declared type metadata", () => {
  const result = projectSymbolSignatures(generation(), { limit: 10 });
  const row = result.signatures.find((entry) => entry.id === "symbol:placeOrder");
  assert.ok(row.signature.includes("OrderService.placeOrder"));
  assert.ok(row.signature.includes("placeOrder(input: OrderInput): Promise<Order>"));
  assert.equal(row.path, "src/order-service.ts");
  assert.equal(row.line, 4);
});

test("cold-start orientation composes bounded entrypoints contracts signatures and weak conventions", () => {
  const result = buildColdStartOrientation(generation(), files, { signatureLimit: 2, entryPointLimit: 4, contractLimit: 4 });
  assert.equal(result.kind, "cold-start-orientation");
  assert.equal(result.generationId, "g-retrieval");
  assert.ok(result.entryPoints.some((entry) => entry.id === "symbol:processPayment"));
  assert.ok(result.contracts.some((contract) => contract.kind === "http" && contract.address === "GET /orders"));
  assert.equal(result.signatures.length, 2);
  assert.ok(Array.isArray(result.repository.topLevelAreas));
});

test("standalone BM25 index returns no result for an empty query", () => {
  const index = new Bm25CodeIndex().replace([{ id: "x", name: "alpha", path: "a.ts" }]);
  assert.deepEqual(index.search(""), []);
});
