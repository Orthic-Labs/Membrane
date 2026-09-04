// D28: compiler/LSP/SCIP adapters and module resolvers — JS/TS + SCIP only
// (second compiler ecosystem is owner-gated). Providers degrade typed, never
// make Blueprint unavailable.

import assert from "node:assert/strict";
import { mkdirSync, mkdtempSync, writeFileSync, rmSync, readFileSync } from "node:fs";
import { tmpdir } from "node:os";
import { dirname, join } from "node:path";
import test from "node:test";
import { fileURLToPath } from "node:url";

import { defineProvider, example } from "../src/providers/index.mjs";
import { pythonScipProvider } from "../src/providers/compilers/python-scip.mjs";
import { resolveModuleSpecifier } from "../src/providers/modules/javascript.mjs";
import { probeScip } from "../src/graph/scip-provider.mjs";

const PYTHON_FIXTURE = join(dirname(fileURLToPath(import.meta.url)), "fixtures", "compiler-adapters", "python");

test("defineProvider enforces the S-20 contract", () => {
  assert.throws(() => defineProvider({ id: "x" }), /missing/);
  assert.equal(example.id, "membrane.typescript");
  assert.equal(example.permissions.filesystem, "repo-read");
  assert.equal(example.permissions.network, "none");
  assert.deepEqual(example.capabilities, ["definitions", "references", "types"]);
});

test("JS module resolver resolves relative, extensionless, and index paths", () => {
  const root = mkdtempSync(join(tmpdir(), "blueprint-resolve-"));
  try {
    mkdirSync(join(root, "src"), { recursive: true });
    mkdirSync(join(root, "src", "lib"), { recursive: true });
    writeFileSync(join(root, "src", "index.ts"), "export const x = 1;\n");
    writeFileSync(join(root, "src", "lib", "util.js"), "module.exports = {};\n");
    const rel = resolveModuleSpecifier({ specifier: "./index", fromFile: join(root, "src", "main.ts") });
    assert.equal(rel.resolved, join(root, "src", "index.ts"));
    const ext = resolveModuleSpecifier({ specifier: "./lib/util", fromFile: join(root, "src", "main.ts") });
    assert.equal(ext.resolved, join(root, "src", "lib", "util.js"));
    const missing = resolveModuleSpecifier({ specifier: "./nope", fromFile: join(root, "src", "main.ts") });
    assert.equal(missing.resolved, null);
    assert.equal(missing.reason, "missing");
  } finally {
    rmSync(root, { recursive: true, force: true });
  }
});

test("JS module resolver abstains on same-tier extension and index ambiguity", () => {
  const root = mkdtempSync(join(tmpdir(), "blueprint-resolve-ambiguous-"));
  try {
    mkdirSync(join(root, "src", "lib"), { recursive: true });
    writeFileSync(join(root, "src", "choice.ts"), "export const value = 'ts';\n");
    writeFileSync(join(root, "src", "choice.js"), "export const value = 'js';\n");
    writeFileSync(join(root, "src", "lib", "index.ts"), "export const value = 'ts';\n");
    writeFileSync(join(root, "src", "lib", "index.js"), "export const value = 'js';\n");

    const extension = resolveModuleSpecifier({
      specifier: "./choice",
      fromFile: join(root, "src", "main.ts"),
      repoRoot: root,
    });
    assert.equal(extension.resolved, null);
    assert.equal(extension.status, "AMBIGUOUS");
    assert.equal(extension.reason, "ambiguous_extension");
    assert.deepEqual(new Set(extension.candidates), new Set([
      join(root, "src", "choice.ts"),
      join(root, "src", "choice.js"),
    ]));

    const index = resolveModuleSpecifier({
      specifier: "./lib",
      fromFile: join(root, "src", "main.ts"),
      repoRoot: root,
    });
    assert.equal(index.resolved, null);
    assert.equal(index.status, "AMBIGUOUS");
    assert.equal(index.reason, "ambiguous_index");
    assert.deepEqual(new Set(index.candidates), new Set([
      join(root, "src", "lib", "index.ts"),
      join(root, "src", "lib", "index.js"),
    ]));

    const exact = resolveModuleSpecifier({
      specifier: "./choice.js",
      fromFile: join(root, "src", "main.ts"),
      repoRoot: root,
    });
    assert.equal(exact.status, "RESOLVED");
    assert.equal(exact.resolutionTier, "exact");
    assert.equal(exact.resolved, join(root, "src", "choice.js"));
  } finally {
    rmSync(root, { recursive: true, force: true });
  }
});

test("JS module resolver resolves bare specifiers through node_modules", () => {
  const root = mkdtempSync(join(tmpdir(), "blueprint-resolve-bare-"));
  try {
    mkdirSync(join(root, "node_modules", "lodash"), { recursive: true });
    writeFileSync(join(root, "node_modules", "lodash", "package.json"), JSON.stringify({ name: "lodash", main: "index.js" }));
    writeFileSync(join(root, "node_modules", "lodash", "index.js"), "module.exports = {};\n");
    const bare = resolveModuleSpecifier({ specifier: "lodash", fromFile: join(root, "src", "main.ts") });
    assert.equal(bare.resolved, join(root, "node_modules", "lodash", "index.js"));
    assert.equal(bare.reason, "package");
  } finally {
    rmSync(root, { recursive: true, force: true });
  }
});

test("SCIP probe degrades typed when absent and reads valid exports", async () => {
  const absent = await probeScip(join(tmpdir(), "no-such-repo"));
  assert.equal(absent.state, "unavailable");
  assert.ok(absent.degradesTo);
  const root = mkdtempSync(join(tmpdir(), "blueprint-scip-"));
  try {
    writeFileSync(join(root, "index.scip.json"), JSON.stringify({ documents: [{ relativePath: "a.ts", occurrences: [] }] }));
    const ok = await probeScip(root);
    assert.ok(ok.state === "ok" || ok.state === "available");
  } finally {
    rmSync(root, { recursive: true, force: true });
  }
});

test("python SCIP provider declares the full compiler contract with repo-read/none/none", () => {
  assert.equal(pythonScipProvider.id, "scip-python");
  assert.equal(pythonScipProvider.kind, "compiler");
  assert.ok(pythonScipProvider.version, "version is set");
  assert.ok(pythonScipProvider.protocolRange, "protocolRange is set");
  assert.deepEqual(pythonScipProvider.capabilities, ["definitions", "references", "types"]);
  assert.equal(pythonScipProvider.permissions.filesystem, "repo-read");
  assert.equal(pythonScipProvider.permissions.network, "none");
  assert.equal(pythonScipProvider.permissions.process, "none");
  assert.ok(Object.isFrozen(pythonScipProvider.capabilities), "capabilities are frozen");
  assert.equal(typeof pythonScipProvider.probe, "function");
  assert.equal(typeof pythonScipProvider.collect, "function");
});

test("python SCIP probe reads the committed fixture index", async () => {
  const probe = await pythonScipProvider.probe({ repoRoot: PYTHON_FIXTURE });
  assert.equal(probe.state, "ok");
  assert.equal(probe.provider, "scip-python");
  assert.equal(probe.precisionTier, "COMPILER");
  assert.equal(probe.indexVersion, "0.6.6");
  assert.equal(probe.documentCount, 3);
  assert.equal(probe.definitionCount, 5);
  assert.equal(probe.referenceCount, 4);
});

test("python SCIP collect resolves the cross-module total() reference to models.Item at COMPILER", async () => {
  const collected = await pythonScipProvider.collect({ repoRoot: PYTHON_FIXTURE });
  assert.equal(collected.index.state, "ok");
  assert.deepEqual(collected.reports, []);
  const itemClass = collected.nodes.find((node) => node.path === "pkg/models.py" && node.symbol?.endsWith("Item#"));
  const totalMethod = collected.nodes.find((node) => node.path === "pkg/models.py" && node.symbol?.endsWith("Item#total()."));
  assert.ok(itemClass, "Item definition node present");
  assert.ok(totalMethod, "Item.total() definition node present");
  assert.equal(itemClass.precisionTier, "COMPILER");
  assert.equal(totalMethod.precisionTier, "COMPILER");
  assert.equal(totalMethod.labels[0], "Method");
  assert.equal(totalMethod.provenance, "AUTHORITATIVE_SEMANTIC");
  assert.equal(totalMethod.confidence, null);
  // pkg/service.py:5 `item.total()` — resolved EXACTLY to the definition in
  // pkg/models.py by SCIP symbol identity (no name match).
  const refs = collected.edges.filter((edge) => edge.kind === "REFERENCES");
  const totalRef = refs.find((edge) => edge.evidence[0].path === "pkg/service.py" && edge.evidence[0].symbol === totalMethod.symbol);
  assert.ok(totalRef, "cross-module total() reference edge present");
  assert.equal(totalRef.target, totalMethod.id);
  assert.equal(totalRef.confidenceTier, "EXACT_RESOLUTION");
  assert.equal(totalRef.provenance, "AUTHORITATIVE_SEMANTIC");
  assert.equal(totalRef.confidence, null);
  assert.equal(totalRef.provider, "scip-python");
  assert.equal(totalRef.precisionTier, "COMPILER");
  // pkg/service.py:0 imports Item — the class reference resolves to the class.
  const classRef = refs.find((edge) => edge.evidence[0].path === "pkg/service.py" && edge.evidence[0].symbol === itemClass.symbol);
  assert.ok(classRef);
  assert.equal(classRef.target, itemClass.id);
  assert.equal(classRef.confidenceTier, "EXACT_RESOLUTION");
  assert.equal(classRef.provenance, "AUTHORITATIVE_SEMANTIC");
  assert.equal(classRef.confidence, null);
});

test("python SCIP collect emits exact TYPED edges for class references", async () => {
  const collected = await pythonScipProvider.collect({ repoRoot: PYTHON_FIXTURE });
  const typed = collected.edges.filter((edge) => edge.kind === "TYPED");
  assert.equal(typed.length, 1);
  assert.equal(typed[0].evidence[0].path, "pkg/service.py");
  assert.equal(typed[0].confidenceTier, "EXACT_RESOLUTION");
  assert.equal(typed[0].precisionTier, "COMPILER");
  assert.equal(typed[0].provenance, "AUTHORITATIVE_SEMANTIC");
  assert.equal(typed[0].confidence, null);
});

test("python SCIP collect reports unknown symbols UNRESOLVED with no speculative match", async () => {
  const collected = await pythonScipProvider.collect({ repoRoot: PYTHON_FIXTURE });
  const unresolved = collected.edges.filter((edge) => edge.resolved === false);
  // main.py imports pkg.service — the module descriptor has no definition
  // anywhere in the index, so it stays UNRESOLVED: kept, never guessed.
  const moduleRef = unresolved.find((edge) => edge.evidence[0].path === "main.py");
  assert.ok(moduleRef, "unknown reference is kept, not dropped");
  assert.equal(moduleRef.target, null);
  assert.equal(moduleRef.confidenceTier, "UNRESOLVED");
  assert.equal(moduleRef.provenance, "UNRESOLVED");
  assert.equal(moduleRef.confidence, null);
  assert.ok(/no definition/.test(moduleRef.reason));
  for (const edge of collected.edges) {
    if (edge.confidenceTier === "UNRESOLVED") assert.equal(edge.target, null);
    else assert.ok(edge.target, `resolved edge must carry a target: ${edge.id}`);
  }
});

test("python SCIP probe degrades typed for absent, unreadable, incompatible, and partial indexes", async () => {
  const absent = await pythonScipProvider.probe({ repoRoot: join(tmpdir(), "no-such-python-repo") });
  assert.equal(absent.state, "unavailable");
  assert.equal(absent.code, "scip_index_absent");
  assert.equal(absent.degradesTo, "AST");

  const root = mkdtempSync(join(tmpdir(), "blueprint-python-scip-"));
  try {
    writeFileSync(join(root, "index.scip.json"), "{ not json");
    const unreadable = await pythonScipProvider.probe({ repoRoot: root });
    assert.equal(unreadable.state, "unavailable");
    assert.equal(unreadable.code, "scip_index_unreadable");
    assert.equal(unreadable.degradesTo, "AST");

    writeFileSync(join(root, "index.scip.json"), JSON.stringify({ nope: [] }));
    const incompatible = await pythonScipProvider.probe({ repoRoot: root });
    assert.equal(incompatible.state, "unavailable");
    assert.equal(incompatible.code, "scip_index_incompatible");
    assert.equal(incompatible.degradesTo, "AST");

    writeFileSync(join(root, "index.scip.json"), JSON.stringify({
      metadata: { version: "9.9.9" },
      documents: [],
    }));
    const versionIncompatible = await pythonScipProvider.probe({ repoRoot: root });
    assert.equal(versionIncompatible.state, "unavailable");
    assert.equal(versionIncompatible.code, "scip_index_version_incompatible");
    assert.equal(versionIncompatible.degradesTo, "AST");

    writeFileSync(join(root, "index.scip.json"), JSON.stringify({
      metadata: { version: "0.6.6" },
      documents: [
        { relativePath: "a.py", occurrences: [] },
        { relativePath: "", occurrences: [{ symbol: "x", roles: ["definition"], range: [0, 0, 0, 1] }] },
      ],
    }));
    const partial = await pythonScipProvider.probe({ repoRoot: root });
    assert.equal(partial.state, "partial");
    assert.equal(partial.code, "scip_index_partial");
    assert.equal(partial.skippedDocuments, 1);
    assert.equal(partial.degradesTo, "AST");
  } finally {
    rmSync(root, { recursive: true, force: true });
  }
});

test("python SCIP collect degrades without throwing and still returns valid entries on a partial index", async () => {
  const absent = await pythonScipProvider.collect({ repoRoot: join(tmpdir(), "no-such-python-repo") });
  assert.deepEqual(absent.nodes, []);
  assert.deepEqual(absent.edges, []);
  assert.equal(absent.reports[0].code, "scip_index_absent");
  assert.equal(absent.reports[0].degradesTo, "AST");
  assert.equal(absent.index.state, "unavailable");

  const root = mkdtempSync(join(tmpdir(), "blueprint-python-scip-partial-"));
  try {
    writeFileSync(join(root, "index.scip.json"), JSON.stringify({
      metadata: { version: "0.6.6", indexer: "scip-python" },
      documents: [
        {
          relativePath: "a.py",
          occurrences: [
            { symbol: "scip-python python fixture 1 a thing().", roles: ["definition"], range: [0, 0, 0, 4] },
            { symbol: "scip-python python fixture 1 a thing().", roles: ["reference"], range: [1, 0, 1, 4] },
          ],
        },
        { relativePath: "", occurrences: [] },
      ],
    }));
    const collected = await pythonScipProvider.collect({ repoRoot: root });
    assert.equal(collected.index.state, "partial");
    assert.ok(collected.reports.some((report) => report.code === "scip_index_partial"));
    assert.ok(collected.nodes.some((node) => node.qualifiedName === "thing"));
    assert.ok(collected.edges.some((edge) => edge.kind === "REFERENCES" && edge.resolved));
  } finally {
    rmSync(root, { recursive: true, force: true });
  }
});

test("python SCIP adapter never spawns a process", () => {
  const source = readFileSync(join(dirname(fileURLToPath(import.meta.url)), "..", "src", "providers", "compilers", "python-scip.mjs"), "utf8");
  for (const banned of ["execFile", "spawn", "exec("]) {
    assert.ok(!source.includes(banned), `adapter must not reference ${banned}`);
  }
});

test("provider absence never makes Blueprint unavailable (typed degradation)", async () => {
  const provider = defineProvider({
    id: "membrane.missing-compiler",
    version: "1.0.0",
    kind: "compiler",
    protocolRange: ">=1 <2",
    capabilities: ["references"],
    permissions: {},
    async probe() { return { state: "unavailable", reason: "compiler_not_installed" }; },
    async collect() { return { nodes: [], edges: [], reports: [{ kind: "unavailable", reason: "compiler_not_installed" }] }; },
  });
  const probe = await provider.probe();
  assert.equal(probe.state, "unavailable");
  const collected = await provider.collect();
  assert.ok(collected.reports.some((r) => r.kind === "unavailable"));
});
