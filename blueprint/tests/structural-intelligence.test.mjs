import assert from "node:assert/strict";
import test from "node:test";

import { augmentStructuralIntelligence, resolveScopedSymbol } from "../src/graph/structural-intelligence.mjs";

function evidence(path, line = 1) { return [{ path, startLine: line, endLine: line, contentHash: `hash:${path}` }]; }
function symbol(id, path, name, qualifiedName, labels) {
  return { id, kind: "symbol", path, name, qualifiedName, labels, confidence: null, evidence: evidence(path) };
}

function fixture() {
  return {
    nodes: [
      { id: "file:src/base.ts", kind: "file", path: "src/base.ts", labels: ["File"], evidence: evidence("src/base.ts") },
      { id: "file:src/child.ts", kind: "file", path: "src/child.ts", labels: ["File"], evidence: evidence("src/child.ts") },
      { id: "file:tests/child.test.ts", kind: "file", path: "tests/child.test.ts", labels: ["File"], evidence: evidence("tests/child.test.ts") },
      symbol("symbol:base:Base", "src/base.ts", "Base", "Base", ["Class"]),
      symbol("symbol:base:run", "src/base.ts", "run", "Base.run", ["Method"]),
      symbol("symbol:child:Child", "src/child.ts", "Child", "Child", ["Class"]),
      symbol("symbol:child:run", "src/child.ts", "run", "Child.run", ["Method"]),
      symbol("symbol:test:child", "tests/child.test.ts", "testChild", "testChild", ["Function"]),
      { id: "domain:event:producer", kind: "domain", path: "src/base.ts", name: "order.created", qualifiedName: "event:order.created", labels: ["EventTopic"], evidence: evidence("src/base.ts") },
      { id: "domain:event:consumer", kind: "domain", path: "src/child.ts", name: "order.created", qualifiedName: "event:order.created", labels: ["EventTopic"], evidence: evidence("src/child.ts") },
    ],
    edges: [
      { id: "import:child-base", kind: "IMPORTS", source: "file:src/child.ts", target: "file:src/base.ts", confidenceTier: "EXACT_RESOLUTION", resolved: true, evidence: evidence("src/child.ts") },
      { id: "call:test-child", kind: "CALLS", source: "symbol:test:child", target: "symbol:child:run", confidenceTier: "EXACT_RESOLUTION", resolved: true, evidence: evidence("tests/child.test.ts") },
      { id: "unresolved:call", kind: "CALLS", source: "symbol:child:run", target: null, confidenceTier: "UNRESOLVED", resolved: false, specifier: "dynamicHandler", reason: "2 candidate symbol(s) named dynamicHandler", candidates: ["symbol:a", "symbol:b"], evidence: evidence("src/child.ts", 4) },
      { id: "produce", kind: "PRODUCES", source: "file:src/base.ts", target: "domain:event:producer", confidenceTier: "CROSS_FILE_HEURISTIC", resolved: true, evidence: evidence("src/base.ts", 3) },
      { id: "consume", kind: "CONSUMES", source: "file:src/child.ts", target: "domain:event:consumer", confidenceTier: "CROSS_FILE_HEURISTIC", resolved: true, evidence: evidence("src/child.ts", 8) },
    ],
  };
}

const files = [
  { path: "src/base.ts", text: "export class Base { run() {} }\nemitter.emit('order.created');\n", contentHash: "hash:src/base.ts" },
  { path: "src/child.ts", text: "import { Base } from './base.js';\nexport class Child extends Base {\n  run() {}\n}\nbus.subscribe('order.created', handler);\n", contentHash: "hash:src/child.ts" },
  { path: "tests/child.test.ts", text: "test('child', () => new Child().run());\n", contentHash: "hash:tests/child.test.ts" },
];

test("scope resolution is exact-first and refuses ambiguous global names", () => {
  const generation = fixture();
  const imported = resolveScopedSymbol(generation, { fromPath: "src/child.ts", name: "Base", typesOnly: true });
  assert.equal(imported.state, "resolved");
  assert.equal(imported.tier, "import");
  assert.equal(imported.symbol.id, "symbol:base:Base");
  generation.nodes.push(symbol("symbol:other:Base", "src/other.ts", "Base", "Base", ["Class"]));
  const fromUnknown = resolveScopedSymbol(generation, { fromPath: "src/unknown.ts", name: "Base", typesOnly: true });
  assert.equal(fromUnknown.state, "ambiguous");
  assert.equal(fromUnknown.reason, "global_name_ambiguous");
});

test("structural intelligence emits hierarchy, overrides, typed frontiers and static test reachability", () => {
  const generation = fixture();
  const summary = augmentStructuralIntelligence(generation, files);
  assert.ok(generation.edges.some((edge) => edge.kind === "INHERITS" && edge.source === "symbol:child:Child" && edge.target === "symbol:base:Base"));
  assert.ok(generation.edges.some((edge) => edge.kind === "OVERRIDES" && edge.source === "symbol:child:run" && edge.target === "symbol:base:run"));
  assert.ok(generation.edges.some((edge) => edge.kind === "TESTS" && edge.source === "symbol:test:child" && edge.target === "symbol:child:run"));
  assert.equal(generation.nodes.find((node) => node.id === "symbol:test:child").entityKind, "test");
  assert.ok(summary.frontiers.some((row) => row.relation === "CALLS" && row.targetName === "dynamicHandler" && row.candidates.length === 2));
  assert.deepEqual(summary.mro.find((row) => row.typeId === "symbol:child:Child").order, ["symbol:base:Base"]);
});

test("literal event producer/consumer share canonical domain identity and gain an explicit dispatch seam", () => {
  const generation = fixture();
  const summary = augmentStructuralIntelligence(generation, files);
  const topic = generation.nodes.find((node) => node.id.startsWith("domain:event-topic:sha256:"));
  assert.ok(topic);
  assert.deepEqual(topic.domainIdentity, { kind: "event_topic", address: "order.created" });
  assert.equal(generation.edges.find((edge) => edge.id === "produce").target, topic.id);
  assert.equal(generation.edges.find((edge) => edge.id === "consume").target, topic.id);
  assert.ok(generation.edges.some((edge) => edge.kind === "HANDLES" && edge.source === topic.id && edge.target === "file:src/child.ts"));
  assert.equal(summary.dynamicDispatchEdges, 1);
});

test("unresolved inheritance becomes a frontier rather than a false exact edge", () => {
  const generation = fixture();
  const localFiles = [...files, { path: "src/missing.ts", text: "class Lost extends UnknownBase {}\n", contentHash: "hash:src/missing.ts" }];
  generation.nodes.push({ id: "file:src/missing.ts", kind: "file", path: "src/missing.ts", labels: ["File"], evidence: evidence("src/missing.ts") });
  generation.nodes.push(symbol("symbol:missing:Lost", "src/missing.ts", "Lost", "Lost", ["Class"]));
  const summary = augmentStructuralIntelligence(generation, localFiles);
  assert.ok(summary.frontiers.some((row) => row.relation === "INHERITS" && row.targetName === "UnknownBase" && row.state === "unresolved"));
  assert.ok(!generation.edges.some((edge) => edge.kind === "INHERITS" && edge.source === "symbol:missing:Lost"));
});
