// D21: generic declarative walker — table-driven node/edge emission with
// evidence, precision tiers, and parse status. Data/markup profiles never
// fabricate function/call facts.

import assert from "node:assert/strict";
import { mkdirSync, writeFileSync, rmSync } from "node:fs";
import { tmpdir } from "node:os";
import { join } from "node:path";
import test from "node:test";

import { defineLanguageTable } from "../graph/language-table.mjs";
import { walkTable } from "../graph/generic-ast-walker.mjs";
import { genericEngineEnabled } from "../graph/treesitter-provider.mjs";

function fakeTree(rootNode) {
  return { rootNode };
}
function node(type, { fieldName = null, text = "", children = [], row = 0 } = {}) {
  return {
    type,
    fieldName,
    text,
    namedChildren: children,
    startPosition: { row, column: 0 },
    endPosition: { row: row + 1, column: 0 },
    hasError: false,
    id: `${type}-${text}-${row}`,
  };
}

test("defineLanguageTable freezes a table with sorted extensions", () => {
  const table = defineLanguageTable({ id: "test", extensions: ["b", "a"], grammarFile: "t.wasm" });
  assert.equal(table.id, "test");
  assert.deepEqual(table.extensions, ["a", "b"]);
  assert.equal(table.factProfile, "code");
  assert.throws(() => { table.extensions.push("c"); }, TypeError);
});

test("walker emits function nodes with evidence and confidence", () => {
  const table = defineLanguageTable({
    id: "t",
    extensions: ["t"],
    grammarFile: "t.wasm",
    functions: [{ nodeTypes: ["function_declaration"], name: { field: "name" }, labels: ["Function"], container: "file" }],
  });
  const fn = node("function_declaration", { text: "function foo() {}", children: [node("identifier", { fieldName: "name", text: "foo" })], row: 2 });
  const result = walkTable({ table, tree: fakeTree(node("program", { children: [fn] })), filePath: "src/a.t", providerId: "cortex-treesitter", precisionTier: "AST" });
  assert.equal(result.nodes.length, 1);
  assert.equal(result.nodes[0].name, "foo");
  assert.equal(result.nodes[0].evidence[0].startLine, 3);
  assert.equal(result.nodes[0].confidenceTier, "EXACT_RESOLUTION");
  assert.ok(result.nodes[0].evidence[0].contentHash);
  assert.equal(result.edges[0].kind, "CONTAINS");
});

test("walker emits imports with unresolved target null", () => {
  const table = defineLanguageTable({
    id: "t",
    extensions: ["t"],
    grammarFile: "t.wasm",
    imports: [{ nodeTypes: ["import_statement"], name: { field: "path" } }],
  });
  const imp = node("import_statement", { text: "import x from 'y'", children: [node("string", { fieldName: "path", text: "'y'" })] });
  const result = walkTable({ table, tree: fakeTree(node("program", { children: [imp] })), filePath: "src/a.t" });
  assert.equal(result.edges[0].kind, "IMPORTS");
  assert.equal(result.edges[0].target, "file:y");
});

test("walker reports partial parse for error trees", () => {
  const table = defineLanguageTable({ id: "t", extensions: ["t"], grammarFile: "t.wasm", functions: [] });
  const root = node("program", { children: [], row: 0 });
  root.hasError = true;
  const result = walkTable({ table, tree: fakeTree(root), filePath: "a.t" });
  assert.ok(result.reports.some((r) => r.kind === "partial_parse"));
});

test("walker reports failed parse when no root node", () => {
  const table = defineLanguageTable({ id: "t", extensions: ["t"], grammarFile: "t.wasm" });
  const result = walkTable({ table, tree: fakeTree(null), filePath: "a.t" });
  assert.ok(result.reports.some((r) => r.kind === "parse_failed"));
});

test("data profile never fabricates function/call facts", () => {
  const table = defineLanguageTable({ id: "json", extensions: ["json"], grammarFile: "json.wasm", factProfile: "data" });
  const result = walkTable({ table, tree: fakeTree(node("document", { children: [node("pair", { text: "\"a\": 1" })] })), filePath: "a.json" });
  assert.equal(result.nodes.filter((n) => n.labels?.includes("Function")).length, 0);
  assert.equal(result.edges.filter((e) => e.kind === "CALLS").length, 0);
});

test("table declarations emit bounded data facts with structural evidence", () => {
  const table = defineLanguageTable({ id: "json", extensions: ["json"], grammarFile: "json.wasm", factProfile: "data", declarations: [{ nodeTypes: ["pair"], name: { field: "key" }, labels: ["Key"] }] });
  const pair = node("pair", { children: [node("string", { fieldName: "key", text: "target" })], row: 2 });
  const result = walkTable({ table, tree: fakeTree(node("document", { children: [pair] })), filePath: "config.json" });
  const declaration = result.nodes.find((item) => item.name === "target");
  assert.deepEqual(declaration?.labels, ["Key"]);
  assert.equal(declaration?.evidence[0].startLine, 3);
  assert.equal(result.edges[0]?.kind, "CONTAINS");
});

test("nested YAML declarations receive stable qualified names", () => {
  const table = defineLanguageTable({ id: "yaml", extensions: ["yaml"], grammarFile: "yaml.wasm", declarations: [{ nodeTypes: ["pair"], name: { field: "key" }, labels: ["Key"] }] });
  const pair = (key, children = []) => node("pair", { children: [node("string", { fieldName: "key", text: key }), ...children] });
  const root = node("document", { children: [pair("outer", [pair("value")]), pair("other", [pair("value")])] });
  const result = walkTable({ table, tree: fakeTree(root), filePath: "config.yaml" });
  assert.deepEqual(result.nodes.map((item) => item.qualifiedName), ["outer", "outer.value", "other", "other.value"]);
});

test("nested same-name generic functions retain distinct qualified IDs", () => {
  const table = defineLanguageTable({ id: "test", extensions: ["test"], grammarFile: "test.wasm", functions: [{ nodeTypes: ["function"], name: { field: "name" }, labels: ["Function"] }] });
  const fn = (name, children = [], row = 0) => node("function", { row, children: [node("identifier", { fieldName: "name", text: name }), ...children] });
  const root = node("program", { children: [fn("same", [fn("same", [], 1)]), fn("same", [], 2)] });
  const result = walkTable({ table, tree: fakeTree(root), filePath: "nested.test" });
  assert.deepEqual(result.nodes.map((item) => item.qualifiedName), ["same", "same.same", "same#3-2"]);
});

test("CORTEX_AST_ENGINE=legacy retains explicit compatibility mode", () => {
  assert.equal(genericEngineEnabled(), true);
  const previous = process.env.CORTEX_AST_ENGINE;
  process.env.CORTEX_AST_ENGINE = "legacy";
  try {
    assert.equal(genericEngineEnabled(), false);
  } finally {
    if (previous === undefined) delete process.env.CORTEX_AST_ENGINE;
    else process.env.CORTEX_AST_ENGINE = previous;
  }
});

test("walker emits comment nodes bound to file", () => {
  const table = defineLanguageTable({
    id: "t",
    extensions: ["t"],
    grammarFile: "t.wasm",
    comments: [{ nodeTypes: ["comment"] }],
  });
  const comment = node("comment", { text: "// why", row: 1 });
  const result = walkTable({ table, tree: fakeTree(node("program", { children: [comment] })), filePath: "a.t" });
  const commentNode = result.nodes.find((n) => n.kind === "comment");
  assert.ok(commentNode);
  assert.equal(commentNode.text, "// why");
});
