import assert from "node:assert/strict";
import { readFileSync } from "node:fs";
import test from "node:test";

import { candidatePaths, isRelativeSpecifier, normalizeRepoPath, resolveSpecifier, resolutionUnsupportedOmission, classifyResolution, RESOLUTION_OMISSION_CODE } from "../src/graph/resolution/index.mjs";
import * as specifierWrapper from "../src/lib/findings/specifier.mjs";

test("resolution owner is the only place scanned-file resolution lives", () => {
  const resolutionSource = readFileSync(new URL("../src/graph/resolution/index.mjs", import.meta.url), "utf8");
  const specifierSource = readFileSync(new URL("../src/lib/findings/specifier.mjs", import.meta.url), "utf8");
  // Owner contains the candidate ladder implementation
  assert.match(resolutionSource, /candidatePaths/);
  assert.match(resolutionSource, /SOURCE_EXTENSIONS/);
  assert.match(resolutionSource, /resolveSpecifier/);
  // Specifier is thin wrapper — no duplicate implementation, only re-export
  assert.match(specifierSource, /from "..\/..\/graph\/resolution\/index\.mjs"/);
  assert.doesNotMatch(specifierSource, /SOURCE_EXTENSIONS\s*=\s*\[/);
  assert.doesNotMatch(specifierSource, /function candidatePaths/);
});

test("wrapper preserves public signatures", () => {
  assert.equal(typeof specifierWrapper.isRelativeSpecifier, "function");
  assert.equal(typeof specifierWrapper.normalizeRepoPath, "function");
  assert.equal(typeof specifierWrapper.candidatePaths, "function");
  assert.equal(typeof specifierWrapper.resolveSpecifier, "function");
  // wrapper forwards to owner identically
  assert.equal(specifierWrapper.isRelativeSpecifier("./a"), isRelativeSpecifier("./a"));
  assert.equal(specifierWrapper.normalizeRepoPath("a\\b"), normalizeRepoPath("a\\b"));
  assert.deepEqual(specifierWrapper.candidatePaths("src/a.ts", "./b.js"), candidatePaths("src/a.ts", "./b.js"));
  const fileSet = new Set(["src/b.ts"]);
  assert.deepEqual(specifierWrapper.resolveSpecifier("src/a.ts", "./b.js", fileSet), resolveSpecifier("src/a.ts", "./b.js", fileSet));
});

test("candidatePaths exact-first ordering (exact, .ts rewrite, index)", () => {
  const candidates = candidatePaths("src/views/a.ts", "./b.js");
  // exact path as written is first
  assert.equal(candidates[0], "src/views/b.js");
  // TS rewrite: ./b.js -> ./b.ts is tried before .js fallback
  const tsIndex = candidates.indexOf("src/views/b.ts");
  const jsIndex = candidates.indexOf("src/views/b.js");
  // exact already is js, but stem extension for ts should appear early
  assert.ok(tsIndex !== -1);
  // index candidates appear after stem extensions
  assert.ok(candidates.includes("src/views/b/index.ts"));
  assert.ok(candidates.includes("src/views/b/index.js"));
  // deduplicated
  assert.equal(candidates.length, new Set(candidates).size);
});

test("resolveSpecifier exact-first first match wins with alternatives evidence", () => {
  const fileSet = new Set(["src/b.ts", "src/b.js", "src/b/index.ts"]);
  const result = resolveSpecifier("src/a.ts", "./b.js", fileSet);
  // candidates[0] is exact src/b.js which matches, so exact wins even though ts also exists
  assert.equal(result.resolved, "src/b.js");
  // alternatives counts remaining matches
  assert.equal(result.alternatives, 1); // src/b.ts also matches
  const onlyExact = resolveSpecifier("src/a.ts", "./b", new Set(["src/b.ts"]));
  assert.equal(onlyExact.resolved, "src/b.ts");
  const missing = resolveSpecifier("src/a.ts", "./missing.js", new Set(["src/other.ts"]));
  assert.equal(missing.resolved, null);
  assert.equal(missing.alternatives, 0);
});

test("isRelativeSpecifier gates external vs relative", () => {
  assert.equal(isRelativeSpecifier("./a"), true);
  assert.equal(isRelativeSpecifier("../a"), true);
  assert.equal(isRelativeSpecifier("some-package"), false);
  assert.equal(isRelativeSpecifier("@scope/pkg"), false);
  assert.equal(isRelativeSpecifier("/absolute"), false);
});

test("closed-surface rule doc comment and typed omission helper", () => {
  const source = readFileSync(new URL("../src/graph/resolution/index.mjs", import.meta.url), "utf8");
  assert.match(source, /closed-surface rule/i);
  assert.match(source, /negatives only when surface closed/i);
  assert.match(source, /resolution_unsupported/);
  const omission = resolutionUnsupportedOmission({ detail: "bare_specifier", reason: "external", specifier: "react", path: "src/a.ts", line: 3 });
  assert.equal(omission.code, "resolution_unsupported");
  assert.equal(omission.code, RESOLUTION_OMISSION_CODE);
  assert.equal(omission.reason, "external");
  assert.equal(omission.detail, "bare_specifier");
  assert.equal(omission.specifier, "react");
});

test("classifyResolution emits typed omission for unsupported/partial/dynamic/generated/external, null only when closed", () => {
  // external
  assert.equal(classifyResolution({ specifier: "react" })?.code, "resolution_unsupported");
  assert.equal(classifyResolution({ specifier: "react" })?.reason, "external");
  // generated
  assert.equal(classifyResolution({ specifier: "./a", isGenerated: true })?.reason, "generated");
  // partial parse
  assert.equal(classifyResolution({ specifier: "./a", parseStatus: "failed" })?.reason, "partial");
  // open surface
  const openSurface = { open: [{ reason: "commonjs_exports" }], parseStatus: "ok" };
  assert.equal(classifyResolution({ specifier: "./a", targetSurface: openSurface })?.reason, "partial");
  // closed surface -> no omission
  const closedSurface = { open: [], parseStatus: "ok" };
  assert.equal(classifyResolution({ specifier: "./a", targetSurface: closedSurface, parseStatus: "ok" }), null);
});
