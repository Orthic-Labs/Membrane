import assert from "node:assert/strict";
import { mkdirSync, mkdtempSync, rmSync, writeFileSync } from "node:fs";
import { tmpdir } from "node:os";
import { join } from "node:path";
import test, { after } from "node:test";
import {
  disposeTreeSitterProvider,
  grammarRuntimeBlockReason,
  PROVIDER,
  SUPPORTED_EXTENSIONS,
  TIER1_LANGUAGE_IDS,
  augmentGeneration,
  buildIncrementalTreeSitterFacts,
  extractFile,
  buildTreeSitterGraph,
  graphCapabilities,
  loadLanguageRecord,
  loadLanguageTable,
  validateGrammarCatalog,
  upgradeDylinkSection,
} from "../graph/treesitter-provider.mjs";

after(() => disposeTreeSitterProvider());

function findNode(nodes, qualifiedName) {
  return nodes.find((node) => node.qualifiedName === qualifiedName);
}

test("provider identity and capability surface are well-formed", () => {
  assert.equal(PROVIDER.id, "cortex-treesitter");
  assert.ok(PROVIDER.version);
  const caps = graphCapabilities();
  assert.equal(caps.provider.id, PROVIDER.id);
  assert.deepEqual(caps.tier1Languages, ["javascript", "python", "rust", "tsx", "typescript"]);
  assert.ok(SUPPORTED_EXTENSIONS.includes("ts"));
  assert.ok(TIER1_LANGUAGE_IDS.includes("rust"));
});

// ---------------------------------------------------------------------------
// Dylink transcoder — unit-level, exercised directly against a synthetic
// legacy section and against a real grammar file (the actual environment bug
// this module works around: tree-sitter-wasms@0.1.13 ships grammars built
// with the pre-"dylink.0" Emscripten dynamic-linking section format, which
// web-tree-sitter@0.26.11 refuses to load as-is).
// ---------------------------------------------------------------------------

test("upgradeDylinkSection transcodes a real grammar file and Language.load succeeds", async () => {
  const { readFileSync } = await import("node:fs");
  const { createRequire } = await import("node:module");
  const wasmPath = createRequire(import.meta.url).resolve("tree-sitter-wasms/out/tree-sitter-javascript.wasm");
  const raw = new Uint8Array(readFileSync(wasmPath));
  const result = upgradeDylinkSection(raw);
  assert.equal(result.upgraded, true);
  assert.equal(result.reason, "ok");
  // The upgraded bytes must actually be loadable — this is the real proof,
  // not just "we transformed some bytes".
  const { Parser, Language } = await import("web-tree-sitter");
  await Parser.init();
  const language = await Language.load(result.bytes);
  assert.ok(language.abiVersion > 0);
});

test("upgradeDylinkSection leaves non-legacy input untouched (never corrupts unknown shapes)", () => {
  const arbitrary = new Uint8Array([0x00, 0x61, 0x73, 0x6d, 0x01, 0x00, 0x00, 0x00, 0x01, 0x00]);
  const result = upgradeDylinkSection(arbitrary);
  assert.equal(result.upgraded, false);
  assert.deepEqual(result.bytes, arbitrary);
});

// ---------------------------------------------------------------------------
// TypeScript — real file, correct symbol extraction.
// ---------------------------------------------------------------------------

test("TypeScript: extracts function, interface, type alias, class, methods, and CONTAINS/DEFINES edges", async () => {
  const file = {
    path: "src/widget.ts",
    text: `import { helper } from "./util";

export function greet(name: string): string {
  return helper(name);
}

export interface Shape { area(): number; }
export type Point = { x: number; y: number };

export class Widget implements Shape {
  radius: number;
  constructor(r: number) { this.radius = r; }
  area(): number { return greet("x").length; }
  static make(r: number): Widget { return new Widget(r); }
}

test("widget greets", () => {
  greet("world");
});
`,
  };
  const result = await extractFile(file);
  assert.equal(result.parseStatus, "ok");
  assert.equal(result.errorNodeCount, 0);
  assert.equal(result.language, "typescript");
  assert.equal(result.provider, PROVIDER.id);
  assert.equal(result.grammar.package, "tree-sitter-wasms");
  assert.ok(result.grammar.hash.startsWith("xxhash128:"));

  const greetNode = findNode(result.nodes, "greet");
  assert.ok(greetNode, "expected a `greet` function node");
  assert.equal(greetNode.kind, "symbol");
  assert.deepEqual(greetNode.labels, ["Function"]);
  assert.equal(greetNode.id, "symbol:src/widget.ts::greet");
  assert.equal(greetNode.confidence, 1);
  // Evidence must be AST-accurate: greet spans lines 3-5 (1-based).
  assert.equal(greetNode.evidence[0].startLine, 3);
  assert.equal(greetNode.evidence[0].endLine, 5);

  const shapeNode = findNode(result.nodes, "Shape");
  assert.deepEqual(shapeNode.labels, ["Interface"]);
  assert.equal(shapeNode.kind, "symbol");

  const pointNode = findNode(result.nodes, "Point");
  assert.deepEqual(pointNode.labels, ["TypeAlias"]);

  const widgetNode = findNode(result.nodes, "Widget");
  assert.equal(widgetNode.kind, "class");
  assert.deepEqual(widgetNode.labels, ["Class"]);

  const areaMethod = findNode(result.nodes, "Widget.area");
  assert.deepEqual(areaMethod.labels, ["Method"]);
  assert.equal(areaMethod.id, "symbol:src/widget.ts::Widget.area");

  const makeMethod = findNode(result.nodes, "Widget.make");
  assert.deepEqual(makeMethod.labels, ["Method"]);

  const testNode = result.nodes.find((node) => node.labels.includes("Test"));
  assert.ok(testNode, "expected a Test-labeled node for the test() call");
  assert.equal(testNode.qualifiedName, "widget greets");

  const containsFromFile = result.edges.filter((e) => e.kind === "CONTAINS" && e.source === "file:src/widget.ts");
  assert.ok(containsFromFile.some((e) => e.target === greetNode.id));
  assert.ok(containsFromFile.some((e) => e.target === widgetNode.id));

  const definesFromWidget = result.edges.filter((e) => e.kind === "DEFINES" && e.source === widgetNode.id);
  assert.ok(definesFromWidget.some((e) => e.target === areaMethod.id));
  assert.ok(definesFromWidget.some((e) => e.target === makeMethod.id));
});

test("TSX: interface + function component + class extend the class-name field correctly", async () => {
  const file = {
    path: "src/Widget.tsx",
    text: `import * as React from "react";
export interface Props { name: string; }
export function Widget(props: Props) {
  return <div>{props.name}</div>;
}
export class Legacy extends React.Component<Props> {
  render() { return <span>{this.props.name}</span>; }
}
`,
  };
  const result = await extractFile(file);
  assert.equal(result.parseStatus, "ok");
  assert.equal(result.language, "tsx");
  assert.ok(findNode(result.nodes, "Props"));
  assert.ok(findNode(result.nodes, "Widget"));
  const legacy = findNode(result.nodes, "Legacy");
  assert.equal(legacy.kind, "class");
  assert.ok(findNode(result.nodes, "Legacy.render"));
});

// ---------------------------------------------------------------------------
// Python — real file.
// ---------------------------------------------------------------------------

test("Python: extracts module-level function, class, methods, pytest-style test, and imports", async () => {
  const file = {
    path: "pkg/widget.py",
    text: `import os
from .util import helper

def plain_fn(x):
    return helper(x)

class Widget:
    def __init__(self):
        self.x = 1

    def render(self):
        return plain_fn(self.x)

def test_widget():
    render()
`,
  };
  const result = await extractFile(file);
  assert.equal(result.parseStatus, "ok");
  assert.equal(result.errorNodeCount, 0);
  assert.equal(result.language, "python");

  const plainFn = findNode(result.nodes, "plain_fn");
  assert.deepEqual(plainFn.labels, ["Function"]);

  const widgetClass = findNode(result.nodes, "Widget");
  assert.equal(widgetClass.kind, "class");

  const initMethod = findNode(result.nodes, "Widget.__init__");
  assert.deepEqual(initMethod.labels, ["Method"]);

  const renderMethod = findNode(result.nodes, "Widget.render");
  assert.deepEqual(renderMethod.labels, ["Method"]);

  const testNode = findNode(result.nodes, "test_widget");
  assert.deepEqual(testNode.labels, ["Test"]);

  assert.equal(result.rawImports.length, 2);
  assert.ok(result.rawImports.some((imp) => imp.specifier === "os"));
  assert.ok(result.rawImports.some((imp) => imp.specifier.includes("util")));
});

// ---------------------------------------------------------------------------
// Rust — real file.
// ---------------------------------------------------------------------------

test("Rust: extracts function, struct, impl methods (qualified), trait, and #[test]-attributed fn", async () => {
  const file = {
    path: "src/widget.rs",
    text: `use crate::util::helper;

pub fn plain_fn(x: i32) -> i32 {
    helper(x)
}

pub struct Widget {
    pub x: i32,
}

impl Widget {
    pub fn new() -> Self {
        Widget { x: 0 }
    }
    pub fn render(&self) -> i32 {
        plain_fn(self.x)
    }
}

trait Shape {
    fn area(&self) -> i32;
}

#[test]
fn test_widget() {
    render();
}
`,
  };
  const result = await extractFile(file);
  assert.equal(result.parseStatus, "ok");
  assert.equal(result.errorNodeCount, 0);
  assert.equal(result.language, "rust");

  const plainFn = findNode(result.nodes, "plain_fn");
  assert.deepEqual(plainFn.labels, ["Function"]);

  const widgetStruct = findNode(result.nodes, "Widget");
  assert.equal(widgetStruct.kind, "class");
  assert.deepEqual(widgetStruct.labels, ["Class", "Struct"]);

  const newMethod = findNode(result.nodes, "Widget.new");
  assert.deepEqual(newMethod.labels, ["Method"]);
  const renderMethod = findNode(result.nodes, "Widget.render");
  assert.deepEqual(renderMethod.labels, ["Method"]);

  const shapeTrait = findNode(result.nodes, "Shape");
  assert.deepEqual(shapeTrait.labels, ["Class", "Trait"]);

  const testNode = findNode(result.nodes, "test_widget");
  assert.deepEqual(testNode.labels, ["Test"]);

  const definesFromWidget = result.edges.filter((e) => e.kind === "DEFINES" && e.source === widgetStruct.id);
  assert.ok(definesFromWidget.some((e) => e.target === newMethod.id));
  assert.ok(definesFromWidget.some((e) => e.target === renderMethod.id));
});

// ---------------------------------------------------------------------------
// Parse-quality honesty — the requirement that matters most: a file with
// parse errors must NOT silently report parseStatus "ok".
// ---------------------------------------------------------------------------

test("a syntactically broken file reports parseStatus 'partial', never 'ok', while still recovering valid symbols", async () => {
  const file = {
    path: "src/broken.ts",
    text: `function ok1() { return 1; }
function broken( {{{ ###
`,
  };
  const result = await extractFile(file);
  assert.notEqual(result.parseStatus, "ok");
  assert.equal(result.parseStatus, "partial");
  assert.ok(result.errorNodeCount > 0);
  const ok1 = findNode(result.nodes, "ok1");
  assert.ok(ok1, "the parser's error recovery should still yield the valid function");
  // Partial-parse symbols are confidence-discounted, not silently full-strength.
  assert.equal(ok1.confidence, 0.6);
});

test("a file that is pure garbage reports parseStatus 'failed' and emits zero symbol nodes", async () => {
  const file = {
    path: "src/garbage.ts",
    text: `@@@ #### $$$ {{{ ]]] ??? !!!`,
  };
  const result = await extractFile(file);
  assert.equal(result.parseStatus, "failed");
  assert.ok(result.errorNodeCount > 0);
  // Only the file node itself — no symbol nodes fabricated from garbage.
  assert.equal(result.nodes.length, 1);
  assert.equal(result.nodes[0].kind, "file");
});

test("an unregistered extension is honestly 'unsupported', not silently 'ok' or 'failed'", async () => {
  const file = { path: "README.md", text: "# hello\n" };
  const result = await extractFile(file);
  assert.equal(result.parseStatus, "unsupported");
  assert.equal(result.language, null);
  assert.equal(result.nodes.length, 1);
});

// ---------------------------------------------------------------------------
// Multi-file repo assembly — cross-file IMPORTS/CALLS/TESTS resolution, and
// honest unresolved-import marking.
// ---------------------------------------------------------------------------

test("buildTreeSitterGraph resolves cross-file imports and calls, and marks unresolved imports honestly", async () => {
  const widget = {
    path: "src/widget.ts",
    text: `import { helper } from "./util";
import { missing } from "./does-not-exist";

export function greet(name: string): string {
  return helper(name);
}
`,
  };
  const util = { path: "src/util.ts", text: `export function helper(name: string): string { return "hi " + name; }\n` };

  const graph = await buildTreeSitterGraph([widget, util]);
  assert.equal(graph.schemaVersion, 1);
  assert.equal(graph.provider.id, PROVIDER.id);
  assert.equal(graph.fileReports.length, 2);
  assert.ok(graph.fileReports.every((report) => report.parseStatus === "ok"));

  const importEdges = graph.edges.filter((e) => e.kind === "IMPORTS");
  const resolvedImport = importEdges.find((e) => e.specifier === "./util");
  assert.ok(resolvedImport);
  assert.equal(resolvedImport.resolved, true);
  assert.equal(resolvedImport.target, "file:src/util.ts");
  assert.equal(resolvedImport.confidence, 1);

  const unresolvedImport = importEdges.find((e) => e.specifier === "./does-not-exist");
  assert.ok(unresolvedImport, "expected an edge record for the unresolvable import");
  assert.equal(unresolvedImport.resolved, false);
  assert.equal(unresolvedImport.target, null, "unresolved import must NOT be emitted as a resolved edge");
  assert.equal(unresolvedImport.confidence, 0);

  const callEdge = graph.edges.find((e) => e.kind === "CALLS" && e.source === "symbol:src/widget.ts::greet");
  assert.ok(callEdge);
  assert.equal(callEdge.target, "symbol:src/util.ts::helper");
});

test("buildTreeSitterGraph resolves TESTS edges from a test-labeled node to the symbol it calls", async () => {
  const file = {
    path: "src/math.ts",
    text: `export function add(a: number, b: number): number { return a + b; }

test("adds numbers", () => {
  add(1, 2);
});

test("Swift degrades only on its proven-crashing Node 24 Linux runtime", () => {
  assert.equal(grammarRuntimeBlockReason("swift", { platform: "linux", nodeVersion: "24.18.0" }), "grammar_runtime_incompatible:swift:linux:node24");
  assert.equal(grammarRuntimeBlockReason("swift", { platform: "darwin", nodeVersion: "24.18.0" }), "grammar_runtime_incompatible:swift:darwin:node24");
  assert.equal(grammarRuntimeBlockReason("swift", { platform: "linux", nodeVersion: "22.22.3" }), null);
  assert.equal(grammarRuntimeBlockReason("swift", { platform: "win32", nodeVersion: "24.18.0" }), null);
  assert.equal(grammarRuntimeBlockReason("ruby", { platform: "linux", nodeVersion: "24.18.0" }), null);
});
`,
  };
  const graph = await buildTreeSitterGraph([file]);
  const testsEdge = graph.edges.find((e) => e.kind === "TESTS");
  assert.ok(testsEdge);
  assert.equal(testsEdge.target, "symbol:src/math.ts::add");
});

test("generic default preserves config-resource facts with canonical provenance", async () => {
  const graph = await buildTreeSitterGraph([
    { path: "src/config.ts", text: "export const orders = 'assets/orders.json';" },
    { path: "assets/orders.json", text: "{}" },
  ]);
  const configured = graph.edges.find((edge) => edge.kind === "CONFIGURES");
  assert.ok(configured, "generic default must retain CONFIGURES facts");
  assert.equal(configured.target, "file:assets/orders.json");
  const configNode = graph.nodes.find((node) => node.qualifiedName === "orders");
  assert.equal(configNode.provider, PROVIDER.id);
  assert.ok(configNode.grammarHash);
  assert.equal(configNode.precisionTier, "AST");
});

test("generic routing derives Go and Ruby from the grammar catalog", async () => {
  for (const file of [
    { path: "pkg/main.go", text: "package pkg\nfunc Hello() {}\n" },
    { path: "lib/hello.rb", text: "def hello\nend\n" },
  ]) {
    const result = await extractFile(file);
    assert.equal(result.generic, true);
    assert.equal(result.parseStatus, "ok");
    assert.ok(result.nodes.some((node) => node.name === "Hello" || node.name === "hello"));
  }
});

test("catalog table and grammar misses remain typed", async () => {
  const table = await loadLanguageTable("not-a-catalog-language");
  assert.equal(table.table, null);
  assert.match(table.error, /^generic_catalog_missing:/);
  const grammar = await loadLanguageRecord("not-a-catalog-language");
  assert.match(grammar.error, /^no grammar registered/);
});

test("augmentation includes catalog-only Go and Ruby files", async () => {
  const root = mkdtempSync(join(tmpdir(), "cortex-catalog-"));
  try {
    mkdirSync(join(root, "pkg"));
    mkdirSync(join(root, "lib"));
    writeFileSync(join(root, "pkg", "main.go"), "package pkg\nfunc Hello() {}\n");
    writeFileSync(join(root, "lib", "hello.rb"), "def hello\nend\n");
    const generation = { nodes: [
      { id: "file:pkg/main.go", kind: "file", path: "pkg/main.go" },
      { id: "file:lib/hello.rb", kind: "file", path: "lib/hello.rb" },
    ], edges: [] };
    const result = await augmentGeneration(generation, root);
    assert.equal(result.summary.candidateFiles, 2);
    assert.equal(result.summary.parsedFiles, 2);
    assert.ok(result.generation.nodes.some((node) => node.qualifiedName === "Hello"));
    assert.ok(result.generation.nodes.some((node) => node.qualifiedName === "hello"));
  } finally { rmSync(root, { recursive: true, force: true }); }
});

test("incremental facts preserve catalog Go symbols for resolution", async () => {
  const facts = await buildIncrementalTreeSitterFacts(
    { path: "pkg/main.go", text: "package pkg\nfunc Hello() {}\n" },
    { symbols: [], filePaths: ["pkg/main.go"] },
  );
  assert.ok(facts.parsed.nodes.some((node) => node.qualifiedName === "Hello"));
  assert.equal(facts.fileReport.parseStatus, "ok");
});

test("JSON catalog table emits canonical key declarations", async () => {
  const result = await extractFile({ path: "config.json", text: "{\"target\": \"asset.json\"}" });
  const key = result.nodes.find((node) => node.qualifiedName === "target");
  assert.deepEqual(key?.labels, ["Key"]);
  assert.equal(key?.evidence[0].path, "config.json");
  assert.equal(key?.confidence, 1);
});

test("full assembly retains comment facts without treating comments as symbols", async () => {
  const graph = await buildTreeSitterGraph([
    { path: "pkg/commented.go", text: "// retained rationale\npackage pkg\nfunc greet() {}\n" },
  ]);
  assert.ok(graph.nodes.some((node) => node.kind === "comment" && node.text.includes("retained rationale")));
  assert.ok(graph.nodes.some((node) => node.qualifiedName === "greet"));
});

test("nested same-name data declarations remain distinct in full and incremental facts", async () => {
  for (const file of [{ path: "config.json", text: '{"outer":{"value":1},"other":{"value":2}}' }]) {
    const full = await buildTreeSitterGraph([file]);
    const incremental = await buildIncrementalTreeSitterFacts(file, { symbols: [], filePaths: [file.path] });
    const fullDeclarations = full.nodes.filter((node) => node.labels?.includes("Key")).map((node) => node.qualifiedName).sort();
    const incrementalDeclarations = incremental.parsed.nodes.filter((node) => node.labels?.includes("Key")).map((node) => node.qualifiedName).sort();
    assert.deepEqual(fullDeclarations, ["other", "other.value", "outer", "outer.value"]);
    assert.deepEqual(incrementalDeclarations, fullDeclarations);
    assert.ok(full.edges.filter((edge) => edge.kind === "CONTAINS").every((edge) => !Array.isArray(edge.evidence?.[0])));
  }
});

test("catalog validation rejects unsafe loader data before import or WASM paths", () => {
  const manifest = { grammars: [{ file: "tree-sitter-safe.wasm" }] };
  for (const catalog of [
    { grammars: [{ language: "../escape", extensions: ["safe"], grammarFile: "tree-sitter-safe.wasm" }] },
    { grammars: [{ language: "safe", extensions: ["../escape"], grammarFile: "tree-sitter-safe.wasm" }] },
    { grammars: [{ language: "safe", extensions: ["safe"], grammarFile: "../escape.wasm" }] },
    { grammars: [{ language: "safe", extensions: ["safe"], grammarFile: "tree-sitter-other.wasm" }] },
  ]) {
    const result = validateGrammarCatalog(catalog, manifest);
    assert.equal(result.ok, false);
    assert.match(result.error, /^generic_catalog_invalid:/);
  }
});
