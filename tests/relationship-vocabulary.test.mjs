// D27: relationship vocabulary, comment claims, and syntax-aware chunks.

import assert from "node:assert/strict";
import test from "node:test";

import { RELATIONSHIP_KINDS, confidenceFromResolutionPath } from "../graph/relationship-kinds.mjs";
import { extractCommentClaims } from "../lib/comment-claims.mjs";
import { chunkBySyntax } from "../lib/token-budget.mjs";

test("relationship vocabulary includes the D27 kinds", () => {
  for (const kind of ["INHERITS", "IMPLEMENTS", "MIXES_IN", "USES", "TESTS", "COVERS", "GENERATES"]) {
    assert.ok(RELATIONSHIP_KINDS.includes(kind), `missing ${kind}`);
  }
});

test("confidence derives from resolution path", () => {
  assert.equal(confidenceFromResolutionPath("compiler"), "EXACT_RESOLUTION");
  assert.equal(confidenceFromResolutionPath("scip"), "EXACT_RESOLUTION");
  assert.equal(confidenceFromResolutionPath("same-file"), "SAME_FILE_LEXICAL");
  assert.equal(confidenceFromResolutionPath("cross-file-heuristic"), "CROSS_FILE_HEURISTIC");
  assert.equal(confidenceFromResolutionPath("guess"), "UNRESOLVED");
});

test("comment claims extract NOTE/WHY/HACK/TODO with span and hash", () => {
  const claims = extractCommentClaims({ text: "// TODO(adrian): add rate limiting", path: "src/a.ts", line: 3, enclosingSymbolId: "symbol:src/a.ts:handle" });
  assert.equal(claims.length, 1);
  const claim = claims[0];
  assert.equal(claim.edges[0].claimKind, "TODO");
  assert.equal(claim.edges[0].owner, "adrian");
  assert.equal(claim.edges[0].enclosingSymbolId, "symbol:src/a.ts:handle");
  assert.equal(claim.edges[0].lifecycle, "data_only");
  assert.ok(claim.sha1);
  assert.equal(claim.source, "src/a.ts");
  assert.equal(claim.line, 3);
});

test("deprecation and owner comments become claims", () => {
  const dep = extractCommentClaims({ text: "// deprecated: use newApi", path: "a.ts", line: 1 });
  assert.ok(dep.some((c) => c.edges[0].claimKind === "DEPRECATED"));
  const owner = extractCommentClaims({ text: "// owner: platform-team", path: "a.ts", line: 2 });
  assert.ok(owner.some((c) => c.edges[0].claimKind === "OWNER"));
});

test("plain comments produce no claims", () => {
  const claims = extractCommentClaims({ text: "// just a note", path: "a.ts", line: 1 });
  assert.equal(claims.length, 0);
});

test("syntax chunks trim at symbol boundaries without cutting", () => {
  const text = [
    "function outer() {",
    "  function inner() {",
    "    return 1;",
    "  }",
    "}",
  ].join("\n");
  const chunk = chunkBySyntax({ text, symbol: { startLine: 2, endLine: 4 }, maxBytes: 1024 });
  assert.equal(chunk.startLine, 2);
  assert.equal(chunk.endLine, 4);
  assert.ok(chunk.text.includes("inner"));
  assert.equal(chunk.receipt, null);
});

test("syntax chunks emit a mandatory truncation receipt when bytes dropped", () => {
  const text = Array.from({ length: 200 }, (_, i) => `line ${i} with some content`).join("\n");
  const chunk = chunkBySyntax({ text, symbol: { startLine: 1, endLine: 200 }, maxBytes: 100 });
  assert.ok(chunk.receipt, "truncation receipt mandatory");
  assert.equal(chunk.receipt.reason, "syntax_chunk_byte_cap");
  assert.ok(chunk.receipt.retainedBytes < chunk.receipt.originalBytes);
  assert.equal(chunk.receipt.symbolBoundary, true);
});

test("syntax chunks use parent span when symbol span is absent", () => {
  const text = ["a", "b", "c", "d"].join("\n");
  const chunk = chunkBySyntax({ text, symbol: { parent: { startLine: 2, endLine: 3 } }, maxBytes: 1024 });
  assert.equal(chunk.startLine, 2);
  assert.equal(chunk.endLine, 3);
  assert.equal(chunk.text, "b\nc");
});
