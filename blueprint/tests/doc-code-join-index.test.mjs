import assert from "node:assert/strict";
import test from "node:test";
import { buildDocCodeJoins } from "../src/graph/static-provider.mjs";

test("doc joins preserve first symbol fallback & first document ownership across edge order", () => {
  const result = buildDocCodeJoins({
    provider: { id: "fixture" },
    nodes: [
      { id: "first", kind: "symbol", path: "src/a.ts" },
      { id: "last", kind: "symbol", path: "src/a.ts" },
    ],
  }, { docMap: {
    nodes: [
      { id: "doc:first", kind: "doc", path: "first.md" },
      { id: "doc:last", kind: "doc", path: "last.md" },
      { id: "claim", kind: "claim", text: "supersedes old.md", line: 2 },
      { id: "ref", kind: "code_ref", path: "src/a.ts" },
    ],
    edges: [
      { type: "contains", from: "doc:last", to: "claim" },
      { type: "contains", from: "doc:first", to: "claim" },
      { type: "mentions-code", from: "doc:first", to: "ref" },
    ],
  } });
  assert.equal(result.joins.length, 1);
  assert.equal(result.joins[0].source, "first");
  assert.equal(result.supersedes.length, 1);
  assert.equal(result.supersedes[0].source.doc, "first.md");
  assert.equal(result.supersedes[0].target, "old.md");
});
