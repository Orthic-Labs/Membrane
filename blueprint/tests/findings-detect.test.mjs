import assert from "node:assert/strict";
import test from "node:test";

import { detectFindings } from "../src/lib/findings/detect.mjs";
import { FINDING_RULE_IDS, FINDING_RULES } from "../src/lib/findings/registry.mjs";
import { candidatePaths, resolveSpecifier } from "../src/lib/findings/specifier.mjs";
import { extractModuleSurface, surfaceIsClosed } from "../src/graph/module-surface.mjs";

function files(map) {
  return Object.entries(map).map(([path, text]) => ({ path, text }));
}

function ruleIds(result) {
  return result.findings.map((finding) => finding.ruleId);
}

function omissionReasons(result) {
  return [...new Set(result.omissions.map((entry) => entry.reason))].sort();
}

// ---------------------------------------------------------------------------
// Registry
// ---------------------------------------------------------------------------

test("registry exposes stable ids with a precision floor and a class", () => {
  assert.deepEqual(FINDING_RULE_IDS, ["BP001", "BP002", "BP003"]);
  for (const id of FINDING_RULE_IDS) {
    const rule = FINDING_RULES[id];
    assert.equal(rule.id, id);
    assert.equal(rule.precisionFloor, "AST");
    assert.ok(["block", "advisory"].includes(rule.class));
    assert.ok(rule.remediation.length > 0);
  }
});

// ---------------------------------------------------------------------------
// Specifier resolution
// ---------------------------------------------------------------------------

test("a .js specifier resolves to its TypeScript source", () => {
  const set = new Set(["src/a.ts", "src/b.ts"]);
  assert.equal(resolveSpecifier("src/a.ts", "./b.js", set).resolved, "src/b.ts");
});

test("a directory specifier resolves through index files", () => {
  const set = new Set(["src/a.ts", "src/thing/index.ts"]);
  assert.equal(resolveSpecifier("src/a.ts", "./thing", set).resolved, "src/thing/index.ts");
});

test("candidate order puts the exact path first", () => {
  assert.equal(candidatePaths("src/a.ts", "./b.json")[0], "src/b.json");
});

// ---------------------------------------------------------------------------
// The clean case — the one that decides whether the channel is trusted
// ---------------------------------------------------------------------------

test("a correct repository produces zero findings", async () => {
  const result = await detectFindings({
    files: files({
      "src/fuse.ts": "export function fuseCandidates() {}\nexport const scoreBatch = () => 1;\n",
      "src/admit.ts": "import { fuseCandidates, scoreBatch } from \"./fuse.js\";\nexport function admit() { return fuseCandidates() ?? scoreBatch(); }\n",
      "src/index.ts": "export { admit } from \"./admit.js\";\nexport * from \"./fuse.js\";\n",
    }),
  });
  assert.deepEqual(result.findings, []);
  assert.equal(result.coverage.filesParsed, 3);
});

// ---------------------------------------------------------------------------
// BP001 — imported binding is not exported
// ---------------------------------------------------------------------------

test("BP001 fires when an imported name is not exported", async () => {
  const result = await detectFindings({
    files: files({
      "src/fuse.ts": "export function fuseCandidates() {}\nexport const scoreBatch = () => 1;\n",
      "src/admit.ts": "import { admitCandidate } from \"./fuse.js\";\nexport const run = () => admitCandidate();\n",
    }),
  });
  assert.deepEqual(ruleIds(result), ["BP001"]);
  const [finding] = result.findings;
  assert.equal(finding.path, "src/admit.ts");
  assert.equal(finding.startLine, 1);
  assert.equal(finding.name, "admitCandidate");
  assert.equal(finding.class, "block");
  assert.deepEqual(finding.evidencePath, ["src/admit.ts", "src/fuse.ts"]);
  assert.match(finding.message, /exports \{ fuseCandidates, scoreBatch \} only/);
});

test("BP001 fires on a missing default export", async () => {
  const result = await detectFindings({
    files: files({
      "src/m.ts": "export const a = 1;\n",
      "src/consumer.ts": "import m from \"./m.js\";\nexport const x = m;\n",
    }),
  });
  assert.deepEqual(ruleIds(result), ["BP001"]);
  assert.equal(result.findings[0].name, "default");
});

test("a present default export is clean", async () => {
  const result = await detectFindings({
    files: files({
      "src/m.ts": "export default function go() {}\n",
      "src/consumer.ts": "import go from \"./m.js\";\nexport const x = go;\n",
    }),
  });
  assert.deepEqual(result.findings, []);
});

test("an aliased import is judged on the exported name, not the local one", async () => {
  const clean = await detectFindings({
    files: files({
      "src/m.ts": "export const realName = 1;\n",
      "src/c.ts": "import { realName as localName } from \"./m.js\";\nexport const x = localName;\n",
    }),
  });
  assert.deepEqual(clean.findings, []);

  const broken = await detectFindings({
    files: files({
      "src/m.ts": "export const realName = 1;\n",
      "src/c.ts": "import { localName as realName } from \"./m.js\";\nexport const x = realName;\n",
    }),
  });
  assert.deepEqual(ruleIds(broken), ["BP001"]);
  assert.equal(broken.findings[0].name, "localName");
});

test("type-only imports of exported types are clean", async () => {
  const result = await detectFindings({
    files: files({
      "src/types.ts": "export type Packet = { id: string };\nexport interface Receipt { id: string }\n",
      "src/use.ts": "import type { Packet, Receipt } from \"./types.js\";\nexport const p: Packet | Receipt = { id: \"1\" };\n",
    }),
  });
  assert.deepEqual(result.findings, []);
});

test("a namespace import never produces a name finding", async () => {
  const result = await detectFindings({
    files: files({
      "src/m.ts": "export const a = 1;\n",
      "src/c.ts": "import * as everything from \"./m.js\";\nexport const x = everything.notThere;\n",
    }),
  });
  assert.deepEqual(result.findings, []);
});

// ---------------------------------------------------------------------------
// BP002 — module not found
// ---------------------------------------------------------------------------

test("BP002 fires when a relative specifier resolves to nothing", async () => {
  const result = await detectFindings({
    files: files({ "src/a.ts": "import { thing } from \"./gone.js\";\nexport const x = thing;\n" }),
  });
  assert.deepEqual(ruleIds(result), ["BP002"]);
  assert.equal(result.findings[0].specifier, "./gone.js");
  assert.equal(result.findings[0].name, null);
});

test("BP002 defers to an out-of-scan target rather than claiming a break", async () => {
  const result = await detectFindings({
    files: files({ "src/a.ts": "import { thing } from \"./generated.js\";\nexport const x = thing;\n" }),
    existsOutsideScan: () => true,
  });
  assert.deepEqual(result.findings, []);
  assert.ok(omissionReasons(result).includes("outside_scanned_set"));
});

test("a bare package specifier is an omission, never a finding", async () => {
  const result = await detectFindings({
    files: files({ "src/a.ts": "import { readFileSync } from \"node:fs\";\nexport const x = readFileSync;\n" }),
  });
  assert.deepEqual(result.findings, []);
  assert.ok(omissionReasons(result).includes("package_specifier"));
});

// ---------------------------------------------------------------------------
// BP003 — barrel re-export break
// ---------------------------------------------------------------------------

test("BP003 fires when a barrel re-exports a name the target lacks", async () => {
  const result = await detectFindings({
    files: files({
      "src/impl.ts": "export function actual() {}\n",
      "src/index.ts": "export { actual, renamed } from \"./impl.js\";\n",
    }),
  });
  assert.deepEqual(ruleIds(result), ["BP003"]);
  assert.equal(result.findings[0].name, "renamed");
  assert.match(result.findings[0].message, /re-exports/);
});

// ---------------------------------------------------------------------------
// Star re-export semantics
// ---------------------------------------------------------------------------

test("a repository-local star re-export chain is followed", async () => {
  const result = await detectFindings({
    files: files({
      "src/deep.ts": "export const deepValue = 1;\n",
      "src/mid.ts": "export * from \"./deep.js\";\n",
      "src/index.ts": "export * from \"./mid.js\";\n",
      "src/c.ts": "import { deepValue } from \"./index.js\";\nexport const x = deepValue;\n",
    }),
  });
  assert.deepEqual(result.findings, []);
});

test("a star re-export does not carry default, and that is reported", async () => {
  const result = await detectFindings({
    files: files({
      "src/impl.ts": "export default function go() {}\nexport const named = 1;\n",
      "src/index.ts": "export * from \"./impl.js\";\n",
      "src/c.ts": "import go from \"./index.js\";\nexport const x = go;\n",
    }),
  });
  assert.deepEqual(ruleIds(result), ["BP001"]);
  assert.equal(result.findings[0].name, "default");
});

test("a star re-export from a package opens the surface and suppresses findings", async () => {
  const result = await detectFindings({
    files: files({
      "src/index.ts": "export * from \"some-package\";\nexport const local = 1;\n",
      "src/c.ts": "import { whoKnows } from \"./index.js\";\nexport const x = whoKnows;\n",
    }),
  });
  assert.deepEqual(result.findings, []);
  assert.ok(omissionReasons(result).includes("open_export_surface"));
});

test("a star re-export cycle is reported, not crashed on", async () => {
  const result = await detectFindings({
    files: files({
      "src/a.ts": "export * from \"./b.js\";\nexport const fromA = 1;\n",
      "src/b.ts": "export * from \"./a.js\";\nexport const fromB = 1;\n",
      "src/c.ts": "import { nope } from \"./a.js\";\nexport const x = nope;\n",
    }),
  });
  assert.deepEqual(result.findings, []);
  assert.ok(omissionReasons(result).includes("star_cycle"));
});

// ---------------------------------------------------------------------------
// Fail-closed suppression — the invariant that keeps the channel trusted
// ---------------------------------------------------------------------------

test("CommonJS module.exports opens the surface", async () => {
  const result = await detectFindings({
    files: files({
      "src/legacy.js": "function helper() {}\nmodule.exports = { helper };\n",
      "src/c.js": "import { helper, other } from \"./legacy.js\";\nexport const x = [helper, other];\n",
    }),
  });
  assert.deepEqual(result.findings, []);
  assert.ok(omissionReasons(result).includes("open_export_surface"));
});

test("a TypeScript export assignment opens the surface", async () => {
  const result = await detectFindings({
    files: files({
      "src/legacy.ts": "declare const thing: number;\nexport = thing;\n",
      "src/c.ts": "import { anything } from \"./legacy.js\";\nexport const x = anything;\n",
    }),
  });
  assert.deepEqual(result.findings, []);
  assert.ok(omissionReasons(result).includes("open_export_surface"));
});

test("a destructured export opens the surface", async () => {
  const result = await detectFindings({
    files: files({
      "src/m.ts": "const source = { a: 1, b: 2 };\nexport const { a, b } = source;\n",
      "src/c.ts": "import { a, missing } from \"./m.js\";\nexport const x = [a, missing];\n",
    }),
  });
  assert.deepEqual(result.findings, []);
  assert.ok(omissionReasons(result).includes("open_export_surface"));
});

test("an unparseable target is an omission, never a finding", async () => {
  const result = await detectFindings({
    files: files({
      "src/broken.ts": "export function ( { { \n",
      "src/c.ts": "import { anything } from \"./broken.js\";\nexport const x = anything;\n",
    }),
  });
  assert.deepEqual(result.findings, []);
  assert.ok(omissionReasons(result).includes("parse_failed"));
});

test("an unsupported target language is an omission", async () => {
  const result = await detectFindings({
    files: files({
      "src/data.json": "{\"a\": 1}\n",
      "src/c.ts": "import { a } from \"./data.json\";\nexport const x = a;\n",
    }),
  });
  assert.deepEqual(result.findings, []);
  assert.ok(omissionReasons(result).includes("unsupported_language"));
});

test("a commented-out import is not a finding", async () => {
  const result = await detectFindings({
    files: files({
      "src/m.ts": "export const a = 1;\n",
      "src/c.ts": "// import { ghost } from \"./gone.js\";\n/* import { ghost2 } from \"./gone2.js\"; */\nimport { a } from \"./m.js\";\nexport const x = a;\n",
    }),
  });
  assert.deepEqual(result.findings, []);
});

test("an import specifier inside a string literal is not a finding", async () => {
  const result = await detectFindings({
    files: files({
      "src/c.ts": "export const doc = `import { ghost } from \"./gone.js\";`;\n",
    }),
  });
  assert.deepEqual(result.findings, []);
});

// ---------------------------------------------------------------------------
// Determinism and identity
// ---------------------------------------------------------------------------

test("a fingerprint survives an unrelated line shift", async () => {
  const before = await detectFindings({
    files: files({
      "src/m.ts": "export const a = 1;\n",
      "src/c.ts": "import { gone } from \"./m.js\";\nexport const x = gone;\n",
    }),
  });
  const after = await detectFindings({
    files: files({
      "src/m.ts": "export const a = 1;\n",
      "src/c.ts": "// a new comment\n// and another\nimport { gone } from \"./m.js\";\nexport const x = gone;\n",
    }),
  });
  assert.equal(before.findings[0].fingerprint, after.findings[0].fingerprint);
  assert.notEqual(before.findings[0].startLine, after.findings[0].startLine);
});

test("findings are ordered deterministically", async () => {
  const input = {
    "src/m.ts": "export const a = 1;\n",
    "src/b.ts": "import { x1 } from \"./m.js\";\nimport { x2 } from \"./m.js\";\nexport const y = [x1, x2];\n",
    "src/a.ts": "import { x3 } from \"./m.js\";\nexport const y = x3;\n",
  };
  const first = await detectFindings({ files: files(input) });
  const second = await detectFindings({ files: files(input) });
  assert.deepEqual(first.findings.map((f) => `${f.path}:${f.startLine}`), second.findings.map((f) => `${f.path}:${f.startLine}`));
  assert.deepEqual(first.findings.map((f) => f.path), ["src/a.ts", "src/b.ts", "src/b.ts"]);
});

// ---------------------------------------------------------------------------
// Surface extraction directly
// ---------------------------------------------------------------------------

test("export surface enumerates every recognised declaration form", async () => {
  const surface = await extractModuleSurface({
    path: "src/all.ts",
    text: [
      "export function fn() {}",
      "export async function afn() {}",
      "export class Cls {}",
      "export abstract class Abs {}",
      "export const one = 1, two = 2;",
      "export type Alias = number;",
      "export interface Iface {}",
      "export enum Enm { A }",
      "export declare const ambient: number;",
      "export default function def() {}",
      "export { one as aliased };",
      "export * as ns from \"./other.js\";",
    ].join("\n"),
  });
  assert.equal(surface.parseStatus, "ok");
  assert.ok(surfaceIsClosed(surface));
  assert.deepEqual([...new Set(surface.exports.map((entry) => entry.name))].sort(),
    ["Abs", "Alias", "Cls", "Enm", "Iface", "afn", "aliased", "ambient", "default", "fn", "ns", "one", "two"]);
});

test("a side-effect import requests no names", async () => {
  const surface = await extractModuleSurface({ path: "src/s.ts", text: "import \"./polyfill.js\";\n" });
  assert.deepEqual(surface.requests, []);
});
